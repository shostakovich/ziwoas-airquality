// Serialisation of the serial line protocol (see SPEC.md). No trailing newline is written.
#pragma once

#include <cstddef>
#include <cstdint>

#include "aggregator.h"

namespace aq {

// All functions write a NUL-terminated compact JSON object into buf and return its length
// (without the NUL). On insufficient capacity they return 0 and buf holds an empty string.
// A nullptr device_id is serialised as null.

size_t format_hello(char* buf, size_t cap, const char* device_id, const char* sensor_fw, const char* firmware);

size_t format_measurement(char* buf, size_t cap, const char* device_id, const Measurement& m,
                          uint32_t device_status);

size_t format_error(char* buf, size_t cap, const char* device_id, const char* message);

// Formats the sensor firmware version as "<major>.<minor>".
size_t format_sensor_fw(char* buf, size_t cap, uint8_t major, uint8_t minor);

}  // namespace aq
