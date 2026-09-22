#include <doctest/doctest.h>

#include <cstring>
#include <string>

#include "json.h"

namespace {

std::string hello(const char* id, const char* fw, const char* firmware) {
    char buf[256];
    const size_t n = aq::format_hello(buf, sizeof buf, id, fw, firmware);
    REQUIRE(n == std::strlen(buf));
    return buf;
}

std::string error(const char* id, const char* msg) {
    char buf[256];
    const size_t n = aq::format_error(buf, sizeof buf, id, msg);
    REQUIRE(n == std::strlen(buf));
    return buf;
}

std::string measurement(const char* id, const aq::Measurement& m, uint32_t status) {
    char buf[320];
    const size_t n = aq::format_measurement(buf, sizeof buf, id, m, status);
    REQUIRE(n == std::strlen(buf));
    return buf;
}

}  // namespace

TEST_CASE("hello line") {
    CHECK(hello("0123456789ABCDEF", "4.0", "v1.2.0-3-gabcdef0-dirty") ==
          R"({"type":"hello","device_id":"0123456789ABCDEF","product":"SEN66","sensor_fw":"4.0","firmware":"v1.2.0-3-gabcdef0-dirty"})");
}

TEST_CASE("sensor firmware version format") {
    char buf[8];
    CHECK(aq::format_sensor_fw(buf, sizeof buf, 4, 0) == 3);
    CHECK(std::string(buf) == "4.0");
    CHECK(aq::format_sensor_fw(buf, sizeof buf, 255, 12) == 6);
    CHECK(std::string(buf) == "255.12");
}

TEST_CASE("full measurement line matches SPEC example") {
    aq::Measurement m;
    m.pm1_0_tenths = 21;
    m.pm2_5_tenths = 34;
    m.pm4_0_tenths = 39;
    m.pm10_tenths = 42;
    m.temperature_tenths = 217;
    m.humidity_tenths = 482;
    m.voc_index = 102;
    m.nox_index = 1;
    m.co2 = 612;
    CHECK(measurement("0123456789ABCDEF", m, 0) ==
          R"({"type":"measurement","device_id":"0123456789ABCDEF","pm1_0":2.1,"pm2_5":3.4,"pm4_0":3.9,"pm10":4.2,"temperature":21.7,"humidity":48.2,"voc_index":102,"nox_index":1,"co2":612,"device_status":0})");
}

TEST_CASE("measurement line with nulls, whole numbers and negatives") {
    aq::Measurement m;
    m.pm1_0_tenths = 0;
    m.pm10_tenths = 65534;
    m.temperature_tenths = -5;
    m.humidity_tenths = 210;
    m.co2 = 400;
    CHECK(measurement("ABC", m, 0xFFFFFFFFu) ==
          R"({"type":"measurement","device_id":"ABC","pm1_0":0.0,"pm2_5":null,"pm4_0":null,"pm10":6553.4,"temperature":-0.5,"humidity":21.0,"voc_index":null,"nox_index":null,"co2":400,"device_status":4294967295})");
    m.temperature_tenths = -215;
    CHECK(measurement("ABC", m, 16).find(R"("temperature":-21.5,)") != std::string::npos);
}

TEST_CASE("all-null measurement") {
    CHECK(measurement("X", aq::Measurement{}, 2097152) ==
          R"({"type":"measurement","device_id":"X","pm1_0":null,"pm2_5":null,"pm4_0":null,"pm10":null,"temperature":null,"humidity":null,"voc_index":null,"nox_index":null,"co2":null,"device_status":2097152})");
}

TEST_CASE("error line with unknown device and escaping") {
    CHECK(error(nullptr, "sensor not found") == R"({"type":"error","device_id":null,"message":"sensor not found"})");
    CHECK(error("ID", "say \"hi\" \\ path\n\r\t\b\f") ==
          R"({"type":"error","device_id":"ID","message":"say \"hi\" \\ path\n\r\t\b\f"})");
    CHECK(error("ID", "\x01\x1f\x7f\xc3\xa4") == "{\"type\":\"error\",\"device_id\":\"ID\",\"message\":\"\\u0001\\u001f\\u007f\\u00c3\\u00a4\"}");
    CHECK(error("q\"id", "") == R"({"type":"error","device_id":"q\"id","message":""})");
}

TEST_CASE("buffer too small yields 0 and an empty string") {
    char buf[32];
    CHECK(aq::format_error(buf, sizeof buf, "ID", "this message is certainly too long for 32 bytes") == 0);
    CHECK(buf[0] == '\0');

    // Exact fit: length + NUL.
    const std::string expected = R"({"type":"error","device_id":null,"message":"x"})";
    char exact[64];
    CHECK(aq::format_error(exact, expected.size() + 1, nullptr, "x") == expected.size());
    CHECK(aq::format_error(exact, expected.size(), nullptr, "x") == 0);
    CHECK(aq::format_error(nullptr, 0, nullptr, "x") == 0);
}
