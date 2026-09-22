# SEN66 → MQTT: Spezifikation

```
SEN66 ──I²C──▶ RP2040 ──USB-CDC, JSON-Zeilen──▶ Bridge (Docker) ──MQTT──▶ Broker ──▶ ZiWoAS
```

Die Logik liegt in der Firmware, die Bridge bleibt dumm. Die Firmware liest, filtert und verdichtet.
Die Bridge validiert, setzt `taken_at` und veröffentlicht. Keine Einheitenumrechnung und keine Aggregation in der Bridge.
Kommentare im Code auf Englisch und sparsam.

## MQTT-Vertrag (verbindlich für ZiWoAS)

| Topic | Payload | retained | QoS |
|---|---|---|---|
| `ziwoas/sen66/<device_id>/state` | JSON-Messung | nein | 1 |
| `ziwoas/sen66/<device_id>/availability` | `online` / `offline` | ja | 1 |

`<device_id>` = SEN66-Seriennummer (`getSerialNumber()`), exakt so, wie die Firmware sie meldet.

State-Payload (Feldnamen nicht umbenennen, kein Feld weglassen, unbekannt = `null`):

```json
{"device_id":"0123456789ABCDEF","taken_at":"2026-09-22T14:03:00Z","pm1_0":2.1,"pm2_5":3.4,"pm4_0":3.9,"pm10":4.2,"temperature":21.7,"humidity":48.2,"voc_index":102,"nox_index":1,"co2":612,"device_status":0}
```

| Feld | Einheit / Typ |
|---|---|
| pm1_0, pm2_5, pm4_0, pm10 | µg/m³, Float, eine Nachkommastelle, oder null |
| temperature | °C, Float, eine Nachkommastelle, oder null |
| humidity | % r. F., Float, eine Nachkommastelle, oder null |
| voc_index, nox_index | Integer 1–500 oder null |
| co2 | ppm, Integer, oder null |
| device_status | Rohwert von `readDeviceStatus()` (uint32) als Integer |
| taken_at | UTC, RFC 3339, auf Sekunden gerundet, Suffix `Z`, von der Bridge beim Empfang gesetzt |

Schlüsselreihenfolge im Payload wie oben (device_id, taken_at, dann Messfelder in Tabellenreihenfolge).

## Serielles Zeilenprotokoll (Firmware → Bridge)

USB-CDC, eine kompakte JSON-Zeile pro Nachricht, `\n`-terminiert (ein `\r` davor muss die Bridge tolerieren).

```
{"type":"hello","device_id":"…","product":"SEN66","sensor_fw":"<major>.<minor>","firmware":"<git-describe>"}
{"type":"measurement","device_id":"…","pm1_0":2.1,"pm2_5":3.4,"pm4_0":3.9,"pm10":4.2,"temperature":21.7,"humidity":48.2,"voc_index":102,"nox_index":1,"co2":612,"device_status":0}
{"type":"error","device_id":"…","message":"…"}
```

- `hello` beim Start, danach alle 10 Minuten und zusätzlich sofort, wenn ein Host den Port öffnet.
- `measurement` enthält alle Felder des State-Payloads außer `taken_at`, in derselben Reihenfolge.
- `error` bei I²C-Fehlern (Text aus `errorToString()`, mit Kontext, welcher Befehl), bei gesetzten Fehlerbits in device_status und bei fehlendem Sensor beim Start. Ist die Seriennummer noch unbekannt, ist `device_id` = `null`.
- JSON-Strings müssen korrekt escaped werden (`"`, `\`, Steuerzeichen).

## Firmware (festgelegt)

- arduino-pico-Core `rp2040:rp2040@6.1.1`, FQBN `rp2040:rp2040:adafruit_feather_adalogger`,
  Bibliotheken `Sensirion I2C SEN66@1.3.1` und `Sensirion Core@0.7.3`, gepinnt in `firmware/sketch.yaml`.
- I²C-Adresse `SEN66_I2C_ADDR_6B` (0x6B).
- Start: `deviceReset()`, ca. 1,2 s warten, `getSerialNumber()`, `getVersion()`, `startContinuousMeasurement()`.
  Danach jede Sekunde `getDataReady()` und bei Bereitschaft `readMeasuredValuesAsIntegers()`.
- **Nicht** `readMeasuredValues()` mit Floats verwenden: Die Float-Variante wandelt die „unbekannt“-Werte stumm in Zahlen um.
  Mit der Integer-Variante erkennt die Firmware „unbekannt“ an den Platzhalterwerten: uint16 `0xFFFF` (PM, CO₂), int16 `0x7FFF`
  (Feuchte, Temperatur, VOC, NOx). VOC/NOx-Index 0 (Anlaufphase des Gas-Index-Algorithmus) gilt ebenfalls als unbekannt. Skalierung: PM /10, Feuchte /100, Temperatur /200, VOC /10, NOx /10, CO₂ ×1.
  Vor der Umsetzung gegen den Bibliotheksheader bzw. das Datenblatt prüfen.
- Fenster 60 s (Konstante, zeitbasiert über `millis()`). Median je Größe über die bekannten Einzelwerte. Bei gerader Anzahl
  der Mittelwert der beiden mittleren Werte. Floats auf eine Nachkommastelle runden, Indizes und CO₂ auf ganze Zahlen
  (kaufmännisch, half away from zero). Ist das ganze Fenster unbekannt, gilt `null`.
- `device_status` = `readDeviceStatus()` am Fensterende. Sind Fehlerbits gesetzt (fanError, rhtError, gasError, co22Error,
  hchoError, pmError, co21Error), gibt die Firmware zusätzlich eine `error`-Zeile aus. Die Warnung `fanSpeedWarning` ist kein Fehler.
- Den Sensor zwischen den Fenstern niemals stoppen. Nach I²C-Fehlern mit Backoff neu anlaufen (z. B. 1 s, 2 s, 4 s … bis max. 60 s),
  nicht hängenbleiben. Ein erneutes `deviceReset()` gibt es nur bei einem echten Wiederanlauf nach Fehlern.
- Kein `while (!Serial)`. Ohne Host weitermessen. Ohne Verbindung darf die Ausgabe verloren gehen, sie darf aber nicht blockieren
  (vor dem Schreiben `Serial.availableForWrite()` bzw. `if (Serial)` prüfen).
- Status-LED optional: blinkt beim Messen, leuchtet dauerhaft bei Fehler.
- CO₂-ASC, Höhe/Luftdruck und Temperatur-Offset bleiben auf Werkseinstellung. Konstanten dafür sind vorbereitet, aber inaktiv.
- Firmware-Version über `-DFIRMWARE_VERSION="…"` aus `git describe --always --dirty --tags`, Fallback `"dev"`.
- microSD bleibt ungenutzt.
- Hardwarefreier Kern (reines C++17, keine Arduino-Header) für Fenster/Median/Null-Behandlung/Rundung/JSON-Serialisierung
  in `firmware/src/core/`. Native Tests mit doctest über CMake in `firmware/test/`.

## Bridge (festgelegt)

- Rust stable, Edition 2024, synchron. Crates: rumqttc (sync `Client`), serialport (`default-features = false`),
  serde/serde_json, humantime, tracing/tracing-subscriber, signal-hook.
- Env: `SERIAL_PORT` (Default `/dev/ttySEN66`), `MQTT_HOST` (Pflicht), `MQTT_PORT` (Default 1883), optional `MQTT_USERNAME`/`MQTT_PASSWORD`,
  `TOPIC_PREFIX` (Default `ziwoas/sen66`), `LOG_LEVEL` (Default `info`).
- MQTT erst verbinden, wenn die Geräte-ID bekannt ist (aus `hello` oder der ersten `measurement`). Last Will `offline`, retained, QoS 1
  auf die Availability-Topic. Ändert sich die Geräte-ID, das alte Gerät offline setzen und mit neuem Last Will neu verbinden.
  Client-ID `sen66_bridge_<device_id>`.
- `hello` bzw. die erste Messung setzt `online` (retained). `measurement` wird validiert (Pflichtfelder vorhanden, Typen passen oder null),
  bekommt `taken_at`, verliert `type` und geht auf `state` (QoS 1, nicht retained).
- `error` und ungültige Zeilen werden nur geloggt. Ungültige Zeilen brechen die Schleife nie ab.
- Port weg (USB gezogen, Lese- oder Öffnungsfehler): `offline` setzen, mit Backoff (1 s … max. 60 s) neu öffnen und nach der
  Wiederkehr bei der nächsten Zeile `online` setzen. Reißt die Broker-Verbindung ab, verbindet rumqttc neu. Messungen dürfen verloren gehen,
  es gibt keinen Puffer.
- SIGTERM/SIGINT: `offline` veröffentlichen, sauber trennen, Exit 0.
- Logging nach stdout, eine Zeile pro Ereignis. Messwerte nur auf DEBUG.
- Tests mit `cargo test`: Parsen und Validieren, Payload inklusive `taken_at`, Müllzeilen, Availability-Übergänge. Port und MQTT per Trait
  und Fakes, keine Hardware.
- Auslieferung: mehrstufiges Dockerfile, statisches musl-Binary, minimales Image (scratch oder distroless-static).
  Broker ohne Auth.

## Deployment

Docker läuft in einer **VM unter Proxmox**. Der USB-Port muss per Vendor:Product-ID an die VM durchgereicht werden.
In Compose immer `/dev/serial/by-id/usb-Adafruit_…-if00:/dev/ttySEN66`, niemals `/dev/ttyACM0`.

## Repo-Struktur

```
firmware/           Arduino-Sketch (firmware.ino), sketch.yaml, build.sh, src/core/ (hardwarefreier Kern)
firmware/test/      doctest-Tests des Kerns (CMake)
bridge/             Rust-Crate, Dockerfile
compose.yml
README.md           Verdrahtung, Flashen, Deployment, MQTT-Vertrag
.github/workflows/  CI: Kern-Tests, cargo test, arduino-cli compile
```

## Abnahmekriterien

- Nach dem Einstecken erscheint innerhalb von 2 Minuten eine gültige Nachricht auf `ziwoas/sen66/<id>/state` (`mosquitto_sub -v -t 'ziwoas/sen66/#'`).
- Nach dem Abziehen steht `availability` binnen weniger Sekunden auf `offline`, nach dem Wiedereinstecken wieder auf `online`, ohne Neustart der Bridge.
- Ein Neustart der Bridge verliert keine Sensor-Lernphase.
- Firmware-Kern- und Bridge-Tests laufen grün in CI.
