// SEN66 -> USB-CDC JSON lines. Protocol and behaviour: see SPEC.md.
// All logic that does not touch hardware lives in src/core/ and is unit-tested on the host.

#include <Arduino.h>
#include <SensirionI2cSen66.h>
#include <Wire.h>

#include "src/core/aggregator.h"
#include "src/core/backoff.h"
#include "src/core/device_status.h"
#include "src/core/json.h"

// Make sure we use the proper definition of NO_ERROR (pattern from the Sensirion examples).
#ifdef NO_ERROR
#undef NO_ERROR
#endif
#define NO_ERROR 0

#ifndef FIRMWARE_VERSION
#define FIRMWARE_VERSION "dev"
#endif

namespace {

constexpr uint32_t kSerialBaud = 115200;
constexpr uint32_t kI2cClockHz = 100000;  // SEN66 maximum
constexpr uint32_t kWindowMs = 60000;
constexpr uint32_t kPollMs = 1000;
constexpr uint32_t kHelloMs = 10UL * 60UL * 1000UL;
constexpr uint32_t kResetSettleMs = 1200;
constexpr uint32_t kStallMs = 10000;         // no new data this long: sensor stopped or was power-cycled
constexpr uint8_t kMaxConsecutiveErrors = 3;  // then a full restart (deviceReset) follows
constexpr uint32_t kBlinkMs = 30;
constexpr size_t kUsbTxFifo = 256;  // CFG_TUD_CDC_TX_BUFSIZE of arduino-pico
// Hosts flush their input right after opening the port (serialport-rs on macOS: tcflush(TCIOFLUSH)
// after open() has already raised DTR), so nothing is sent until the connection has settled.
constexpr uint32_t kHostSettleMs = 500;

// Prepared but INACTIVE: the sensor stays at factory settings (SPEC.md). Nothing below is sent to the sensor.
[[maybe_unused]] constexpr uint16_t kCo2AutoSelfCalibration = 1;  // setCo2SensorAutomaticSelfCalibration(), 1 = on
[[maybe_unused]] constexpr uint16_t kSensorAltitudeM = 0;         // setSensorAltitude(), metres
[[maybe_unused]] constexpr uint16_t kAmbientPressureHpa = 1013;   // setAmbientPressure(), hPa (overrides altitude)
[[maybe_unused]] constexpr int16_t kTemperatureOffset = 0;        // setTemperatureOffsetParameters(offset * 200, ...)

SensirionI2cSen66 sensor;
aq::WindowAggregator window;
aq::Backoff backoff;

char serialNumber[33] = {0};
bool haveSerialNumber = false;
char sensorFw[8] = "?";

bool running = false;          // sensor initialised and measuring
bool everStarted = false;
uint32_t retryAtMs = 0;        // when not running: start of the current wait
uint32_t retryWaitMs = 0;
uint8_t consecutiveErrors = 0;
uint32_t pollIntervalMs = kPollMs;
uint32_t lastPollMs = 0;
uint32_t lastDataMs = 0;
uint32_t windowStartMs = 0;
uint32_t lastHelloMs = 0;
bool hostWasConnected = false;
bool helloPending = false;
uint32_t hostConnectedAtMs = 0;
bool statusErrors = false;
uint32_t blinkStartMs = 0;
bool blinking = false;

const char* deviceId() { return haveSerialNumber ? serialNumber : nullptr; }

// Never blocks on a missing or stalled host: lines that do not fit are dropped.
void writeLine(const char* line, size_t len) {
    if (len == 0 || !Serial) return;
    const size_t need = (len + 1 < kUsbTxFifo) ? len + 1 : kUsbTxFifo;
    if (static_cast<size_t>(Serial.availableForWrite()) < need) return;
    Serial.write(reinterpret_cast<const uint8_t*>(line), len);
    Serial.write('\n');
}

void emitHello(uint32_t now) {
    lastHelloMs = now;
    helloPending = false;
    char buf[192];
    writeLine(buf, aq::format_hello(buf, sizeof buf, deviceId(), sensorFw, FIRMWARE_VERSION));
}

void emitError(const char* message) {
    char buf[256];
    writeLine(buf, aq::format_error(buf, sizeof buf, deviceId(), message));
}

void emitI2cError(const char* prefix, const char* command, int16_t error) {
    char text[64];
    errorToString(error, text, sizeof text);
    char message[160];
    snprintf(message, sizeof message, "%s%s(): %s", prefix, command, text);
    emitError(message);
}

void scheduleRestart(uint32_t now) {
    running = false;
    retryAtMs = now;
    retryWaitMs = backoff.next();
}

bool startSensor() {
    const char* prefix = everStarted ? "restart failed: " : "SEN66 not found at startup: ";
    int16_t error = sensor.deviceReset();
    if (error != NO_ERROR) {
        emitI2cError(prefix, "deviceReset", error);
        return false;
    }
    delay(kResetSettleMs);

    int8_t sn[sizeof serialNumber] = {0};
    error = sensor.getSerialNumber(sn, sizeof sn - 1);
    if (error != NO_ERROR) {
        emitI2cError(prefix, "getSerialNumber", error);
        return false;
    }
    memcpy(serialNumber, sn, sizeof serialNumber - 1);
    serialNumber[sizeof serialNumber - 1] = '\0';
    haveSerialNumber = true;

    uint8_t major = 0;
    uint8_t minor = 0;
    error = sensor.getVersion(major, minor);
    if (error != NO_ERROR) {
        emitI2cError(prefix, "getVersion", error);
        return false;
    }
    aq::format_sensor_fw(sensorFw, sizeof sensorFw, major, minor);

    error = sensor.startContinuousMeasurement();
    if (error != NO_ERROR) {
        emitI2cError(prefix, "startContinuousMeasurement", error);
        return false;
    }
    return true;
}

void attemptStart() {
    if (!startSensor()) {
        scheduleRestart(millis());
        return;
    }
    const uint32_t now = millis();
    running = true;
    everStarted = true;
    consecutiveErrors = 0;
    pollIntervalMs = kPollMs;
    lastPollMs = now;
    lastDataMs = now;
    windowStartMs = now;
    window.reset();
    emitHello(now);
}

void onI2cError(const char* command, int16_t error, uint32_t now) {
    emitI2cError("", command, error);
    if (++consecutiveErrors >= kMaxConsecutiveErrors) {
        emitError("too many I2C errors, restarting sensor");
        scheduleRestart(now);
        return;
    }
    pollIntervalMs = backoff.next();
}

void poll(uint32_t now) {
    lastPollMs = now;
    uint8_t padding = 0;
    bool dataReady = false;
    int16_t error = sensor.getDataReady(padding, dataReady);
    if (error != NO_ERROR) {
        onI2cError("getDataReady", error, now);
        return;
    }
    if (!dataReady) {
        if (aq::elapsed(now, lastDataMs, kStallMs)) {
            emitError("no new data for 10 s, restarting sensor");
            scheduleRestart(now);
        }
        return;
    }

    aq::RawSample s{};
    error = sensor.readMeasuredValuesAsIntegers(s.pm1_0, s.pm2_5, s.pm4_0, s.pm10, s.humidity, s.temperature,
                                                s.voc_index, s.nox_index, s.co2);
    if (error != NO_ERROR) {
        onI2cError("readMeasuredValuesAsIntegers", error, now);
        return;
    }
    window.add(s);
    lastDataMs = now;
    consecutiveErrors = 0;
    pollIntervalMs = kPollMs;
    backoff.reset();
    blinking = true;
    blinkStartMs = now;
}

void finishWindow(uint32_t now) {
    windowStartMs = now;
    if (window.sample_count() == 0) return;  // nothing measured; errors were already reported

    SEN66DeviceStatus status{};
    int16_t error = sensor.readDeviceStatus(status);
    if (error != NO_ERROR) error = sensor.readDeviceStatus(status);  // one retry before dropping the window
    if (error != NO_ERROR) {
        window.reset();
        onI2cError("readDeviceStatus", error, now);
        return;
    }

    char buf[320];
    writeLine(buf, aq::format_measurement(buf, sizeof buf, deviceId(), window.result(), status.value));
    window.reset();

    statusErrors = aq::has_device_errors(status.value);
    if (statusErrors) {
        char message[128];
        aq::describe_device_errors(message, sizeof message, status.value);
        emitError(message);
    }
}

void updateLed(uint32_t now) {
#ifdef LED_BUILTIN
    const bool error = !running || consecutiveErrors > 0 || statusErrors;
    if (blinking && aq::elapsed(now, blinkStartMs, kBlinkMs)) blinking = false;
    digitalWrite(LED_BUILTIN, (error || blinking) ? HIGH : LOW);
#else
    (void)now;
#endif
}

}  // namespace

void setup() {
    Serial.begin(kSerialBaud);  // no while (!Serial): measure without a host
#ifdef LED_BUILTIN
    pinMode(LED_BUILTIN, OUTPUT);
    digitalWrite(LED_BUILTIN, HIGH);
#endif
    Wire.begin();
    Wire.setClock(kI2cClockHz);
    sensor.begin(Wire, SEN66_I2C_ADDR_6B);
    attemptStart();
}

void loop() {
    const uint32_t now = millis();
    updateLed(now);

    // A host (re)opening the port gets a hello once the connection has settled,
    // instead of waiting up to 10 minutes.
    const bool hostConnected = static_cast<bool>(Serial);
    if (hostConnected && !hostWasConnected) {
        hostConnectedAtMs = now;
        helloPending = true;
    }
    hostWasConnected = hostConnected;
    const bool hostSettling = hostConnected && !aq::elapsed(now, hostConnectedAtMs, kHostSettleMs);
    if (helloPending && hostConnected && !hostSettling && running) emitHello(now);

    if (!running) {
        if (aq::elapsed(now, retryAtMs, retryWaitMs)) attemptStart();
        return;
    }
    if (!hostSettling && aq::elapsed(now, lastHelloMs, kHelloMs)) emitHello(now);
    if (aq::elapsed(now, lastPollMs, pollIntervalMs)) poll(now);
    // A window ending while a host is settling is closed up to kHostSettleMs late rather than lost.
    if (running && !hostSettling && aq::elapsed(now, windowStartMs, kWindowMs)) finishWindow(now);
}
