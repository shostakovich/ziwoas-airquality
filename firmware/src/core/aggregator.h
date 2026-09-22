// Hardware-free window aggregation for SEN66 integer readings (C++17, no Arduino headers).
#pragma once

#include <cstddef>
#include <cstdint>
#include <optional>

namespace aq {

// Placeholder values the SEN66 reports for "unknown".
constexpr uint16_t kUnknownU16 = 0xFFFF;
constexpr int16_t kUnknownI16 = 0x7FFF;

// VOC/NOx index (raw, x10). The gas index algorithm reports 0 during its
// start-up blackout; valid indices are 1..500, so 0 also means "unknown".
constexpr bool is_known_gas_index(int16_t raw) { return raw != kUnknownI16 && raw > 0; }

// One reading exactly as returned by readMeasuredValuesAsIntegers().
struct RawSample {
    uint16_t pm1_0;
    uint16_t pm2_5;
    uint16_t pm4_0;
    uint16_t pm10;
    int16_t humidity;     // %RH * 100
    int16_t temperature;  // degC * 200
    int16_t voc_index;    // index * 10
    int16_t nox_index;    // index * 10
    uint16_t co2;         // ppm
};

// Aggregated window result. std::nullopt means "unknown" (JSON null).
// Float quantities are stored as integer tenths to keep rounding exact.
struct Measurement {
    std::optional<int32_t> pm1_0_tenths;
    std::optional<int32_t> pm2_5_tenths;
    std::optional<int32_t> pm4_0_tenths;
    std::optional<int32_t> pm10_tenths;
    std::optional<int32_t> temperature_tenths;
    std::optional<int32_t> humidity_tenths;
    std::optional<int32_t> voc_index;
    std::optional<int32_t> nox_index;
    std::optional<int32_t> co2;
};

// num / den rounded half away from zero; den must be > 0.
int64_t div_round_half_away(int64_t num, int64_t den);

// Twice the median of values[0..n) (so even counts stay exact); sorts values in place.
// Returns nullopt for n == 0.
std::optional<int64_t> median_x2(int32_t* values, size_t n);

class WindowAggregator {
  public:
    // Generous for a 60 s window at 1 Hz; extra samples are dropped.
    static constexpr size_t kCapacity = 128;

    void reset();
    void add(const RawSample& s);
    size_t sample_count() const { return samples_; }
    Measurement result() const;

  private:
    struct Series {
        int32_t values[kCapacity];
        size_t count = 0;
        void push(int32_t v) {
            if (count < kCapacity) values[count++] = v;
        }
        std::optional<int64_t> median_x2() const;
    };

    Series pm1_0_, pm2_5_, pm4_0_, pm10_;
    Series humidity_, temperature_, voc_, nox_, co2_;
    size_t samples_ = 0;
};

}  // namespace aq
