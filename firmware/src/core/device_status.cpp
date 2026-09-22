#include "device_status.h"

namespace aq {

size_t describe_device_errors(char* buf, size_t cap, uint32_t status) {
    size_t len = 0;
    bool overflow = false;
    auto put = [&](char c) {
        if (len + 1 < cap) {
            buf[len++] = c;
        } else {
            overflow = true;
        }
    };
    auto put_str = [&](const char* s) {
        while (*s) put(*s++);
    };

    put_str("device_status error bits: ");
    bool first = true;
    for (const StatusBit& b : kDeviceStatusErrors) {
        if (status & (1u << b.bit)) {
            if (!first) put(',');
            first = false;
            put_str(b.name);
        }
    }
    if (first) put_str("none");
    put_str(" (0x");
    static const char kHex[] = "0123456789ABCDEF";
    for (int shift = 28; shift >= 0; shift -= 4) put(kHex[(status >> shift) & 0xF]);
    put(')');

    if (cap == 0) return 0;
    if (overflow) {
        buf[0] = '\0';
        return 0;
    }
    buf[len] = '\0';
    return len;
}

}  // namespace aq
