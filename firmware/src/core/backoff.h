// Exponential retry delay: 1 s, 2 s, 4 s ... capped at 60 s.
#pragma once

#include <cstdint>

namespace aq {

class Backoff {
  public:
    static constexpr uint32_t kInitialMs = 1000;
    static constexpr uint32_t kMaxMs = 60000;

    // Returns the delay to wait now and doubles the next one.
    uint32_t next() {
        const uint32_t d = delay_;
        delay_ = (delay_ >= kMaxMs / 2) ? kMaxMs : delay_ * 2;
        return d;
    }
    void reset() { delay_ = kInitialMs; }

  private:
    uint32_t delay_ = kInitialMs;
};

// Wrap-safe check whether interval_ms has elapsed since start_ms (millis() wraps after ~49 days).
constexpr bool elapsed(uint32_t now_ms, uint32_t start_ms, uint32_t interval_ms) {
    return static_cast<uint32_t>(now_ms - start_ms) >= interval_ms;
}

}  // namespace aq
