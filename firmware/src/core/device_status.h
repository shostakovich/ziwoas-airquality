// SEN66 device status evaluation (bit layout of SEN66DeviceStatus in SensirionI2cSen66.h 1.3.1).
#pragma once

#include <cstddef>
#include <cstdint>

namespace aq {

struct StatusBit {
    uint8_t bit;
    const char* name;
};

// Error flags; fanSpeedWarning (bit 21) is intentionally not listed.
constexpr StatusBit kDeviceStatusErrors[] = {
    {4, "fanError"},   {6, "rhtError"},  {7, "gasError"},   {9, "co22Error"},
    {10, "hchoError"}, {11, "pmError"},  {12, "co21Error"},
};

constexpr uint32_t kDeviceStatusErrorMask = (1u << 4) | (1u << 6) | (1u << 7) | (1u << 9) | (1u << 10) |
                                            (1u << 11) | (1u << 12);

constexpr bool has_device_errors(uint32_t status) { return (status & kDeviceStatusErrorMask) != 0; }

// Writes e.g. "device_status error bits: fanError,pmError (0x00000810)". Returns length, 0 if it did not fit.
size_t describe_device_errors(char* buf, size_t cap, uint32_t status);

}  // namespace aq
