#include "json.h"

namespace aq {

namespace {

class Writer {
  public:
    Writer(char* buf, size_t cap) : buf_(buf), cap_(cap) {}

    void ch(char c) {
        // Keep one byte for the terminating NUL.
        if (len_ + 1 < cap_) {
            buf_[len_++] = c;
        } else {
            overflow_ = true;
        }
    }

    void raw(const char* s) {
        while (*s) ch(*s++);
    }

    void write_u64(uint64_t v) {
        char digits[20];
        size_t n = 0;
        do {
            digits[n++] = static_cast<char>('0' + v % 10);
            v /= 10;
        } while (v != 0);
        while (n > 0) ch(digits[--n]);
    }

    void integer(int64_t v) {
        if (v < 0) {
            ch('-');
            write_u64(static_cast<uint64_t>(-(v + 1)) + 1);
        } else {
            write_u64(static_cast<uint64_t>(v));
        }
    }

    void tenths(int32_t t) {
        uint64_t magnitude = t < 0 ? static_cast<uint64_t>(-static_cast<int64_t>(t)) : static_cast<uint64_t>(t);
        if (t < 0) ch('-');
        write_u64(magnitude / 10);
        ch('.');
        ch(static_cast<char>('0' + magnitude % 10));
    }

    void string(const char* s) {
        if (s == nullptr) {
            raw("null");
            return;
        }
        static const char kHex[] = "0123456789abcdef";
        ch('"');
        for (; *s; ++s) {
            const unsigned char c = static_cast<unsigned char>(*s);
            switch (c) {
                case '"': raw("\\\""); break;
                case '\\': raw("\\\\"); break;
                case '\n': raw("\\n"); break;
                case '\r': raw("\\r"); break;
                case '\t': raw("\\t"); break;
                case '\b': raw("\\b"); break;
                case '\f': raw("\\f"); break;
                default:
                    // Control characters and non-ASCII bytes are escaped so a line is always valid UTF-8.
                    if (c < 0x20 || c >= 0x7F) {
                        raw("\\u00");
                        ch(kHex[c >> 4]);
                        ch(kHex[c & 0x0F]);
                    } else {
                        ch(static_cast<char>(c));
                    }
            }
        }
        ch('"');
    }

    void key(const char* k) {
        ch(first_ ? '{' : ',');
        first_ = false;
        ch('"');
        raw(k);
        raw("\":");
    }

    void opt_tenths(const char* k, const std::optional<int32_t>& v) {
        key(k);
        if (v) {
            tenths(*v);
        } else {
            raw("null");
        }
    }

    void opt_integer(const char* k, const std::optional<int32_t>& v) {
        key(k);
        if (v) {
            integer(*v);
        } else {
            raw("null");
        }
    }

    size_t finish() {
        if (!first_) ch('}');
        if (cap_ == 0) return 0;
        if (overflow_) {
            buf_[0] = '\0';
            return 0;
        }
        buf_[len_] = '\0';
        return len_;
    }

  private:
    char* buf_;
    size_t cap_;
    size_t len_ = 0;
    bool overflow_ = false;
    bool first_ = true;
};

}  // namespace

size_t format_hello(char* buf, size_t cap, const char* device_id, const char* sensor_fw, const char* firmware) {
    Writer w(buf, cap);
    w.key("type");
    w.string("hello");
    w.key("device_id");
    w.string(device_id);
    w.key("product");
    w.string("SEN66");
    w.key("sensor_fw");
    w.string(sensor_fw);
    w.key("firmware");
    w.string(firmware);
    return w.finish();
}

size_t format_measurement(char* buf, size_t cap, const char* device_id, const Measurement& m,
                          uint32_t device_status) {
    Writer w(buf, cap);
    w.key("type");
    w.string("measurement");
    w.key("device_id");
    w.string(device_id);
    w.opt_tenths("pm1_0", m.pm1_0_tenths);
    w.opt_tenths("pm2_5", m.pm2_5_tenths);
    w.opt_tenths("pm4_0", m.pm4_0_tenths);
    w.opt_tenths("pm10", m.pm10_tenths);
    w.opt_tenths("temperature", m.temperature_tenths);
    w.opt_tenths("humidity", m.humidity_tenths);
    w.opt_integer("voc_index", m.voc_index);
    w.opt_integer("nox_index", m.nox_index);
    w.opt_integer("co2", m.co2);
    w.key("device_status");
    w.write_u64(device_status);
    return w.finish();
}

size_t format_error(char* buf, size_t cap, const char* device_id, const char* message) {
    Writer w(buf, cap);
    w.key("type");
    w.string("error");
    w.key("device_id");
    w.string(device_id);
    w.key("message");
    w.string(message);
    return w.finish();
}

size_t format_sensor_fw(char* buf, size_t cap, uint8_t major, uint8_t minor) {
    // Not JSON, but the same bounded writer is handy.
    Writer w(buf, cap);
    w.write_u64(major);
    w.ch('.');
    w.write_u64(minor);
    return w.finish();
}

}  // namespace aq
