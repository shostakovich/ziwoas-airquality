#include <doctest/doctest.h>

#include <string>

#include "device_status.h"

TEST_CASE("error bit mask follows SEN66DeviceStatus") {
    CHECK(aq::kDeviceStatusErrorMask == 0x00001ED0u);
    CHECK_FALSE(aq::has_device_errors(0));
    CHECK_FALSE(aq::has_device_errors(1u << 21));  // fanSpeedWarning is only a warning
    CHECK_FALSE(aq::has_device_errors(0xFu | (1u << 5) | (1u << 8)));  // reserved bits
    for (unsigned bit : {4u, 6u, 7u, 9u, 10u, 11u, 12u}) {
        CAPTURE(bit);
        CHECK(aq::has_device_errors(1u << bit));
    }
}

TEST_CASE("describe_device_errors") {
    char buf[160];
    CHECK(aq::describe_device_errors(buf, sizeof buf, (1u << 4) | (1u << 11) | (1u << 21)) > 0);
    CHECK(std::string(buf) == "device_status error bits: fanError,pmError (0x00200810)");
    aq::describe_device_errors(buf, sizeof buf, 0x1ED0);
    CHECK(std::string(buf) ==
          "device_status error bits: fanError,rhtError,gasError,co22Error,hchoError,pmError,co21Error (0x00001ED0)");
    aq::describe_device_errors(buf, sizeof buf, 0);
    CHECK(std::string(buf) == "device_status error bits: none (0x00000000)");
    char tiny[8];
    CHECK(aq::describe_device_errors(tiny, sizeof tiny, 0x10) == 0);
    CHECK(tiny[0] == '\0');
}
