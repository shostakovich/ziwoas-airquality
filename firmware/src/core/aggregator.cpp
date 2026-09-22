#include "aggregator.h"

#include <algorithm>
#include <initializer_list>

namespace aq {

int64_t div_round_half_away(int64_t num, int64_t den) {
    // (2|num| + den) / (2 den) rounds |num|/den half up; the sign is reapplied afterwards.
    const bool negative = num < 0;
    const int64_t magnitude = negative ? -num : num;
    const int64_t q = (2 * magnitude + den) / (2 * den);
    return negative ? -q : q;
}

std::optional<int64_t> median_x2(int32_t* values, size_t n) {
    if (n == 0) return std::nullopt;
    std::sort(values, values + n);
    if (n % 2 == 1) return 2 * static_cast<int64_t>(values[n / 2]);
    return static_cast<int64_t>(values[n / 2 - 1]) + values[n / 2];
}

std::optional<int64_t> WindowAggregator::Series::median_x2() const {
    int32_t scratch[kCapacity];
    std::copy(values, values + count, scratch);
    return aq::median_x2(scratch, count);
}

void WindowAggregator::reset() {
    for (Series* s : {&pm1_0_, &pm2_5_, &pm4_0_, &pm10_, &humidity_, &temperature_, &voc_, &nox_, &co2_}) {
        s->count = 0;
    }
    samples_ = 0;
}

void WindowAggregator::add(const RawSample& s) {
    if (samples_ >= kCapacity) return;
    ++samples_;
    if (s.pm1_0 != kUnknownU16) pm1_0_.push(s.pm1_0);
    if (s.pm2_5 != kUnknownU16) pm2_5_.push(s.pm2_5);
    if (s.pm4_0 != kUnknownU16) pm4_0_.push(s.pm4_0);
    if (s.pm10 != kUnknownU16) pm10_.push(s.pm10);
    if (s.humidity != kUnknownI16) humidity_.push(s.humidity);
    if (s.temperature != kUnknownI16) temperature_.push(s.temperature);
    if (is_known_gas_index(s.voc_index)) voc_.push(s.voc_index);
    if (is_known_gas_index(s.nox_index)) nox_.push(s.nox_index);
    if (s.co2 != kUnknownU16) co2_.push(s.co2);
}

namespace {

// median_x2 is twice the raw median, so value = median_x2 / (2 * scale).
std::optional<int32_t> to_tenths(std::optional<int64_t> m2, int64_t scale) {
    if (!m2) return std::nullopt;
    return static_cast<int32_t>(div_round_half_away(*m2 * 10, 2 * scale));
}

std::optional<int32_t> to_integer(std::optional<int64_t> m2, int64_t scale) {
    if (!m2) return std::nullopt;
    return static_cast<int32_t>(div_round_half_away(*m2, 2 * scale));
}

}  // namespace

Measurement WindowAggregator::result() const {
    Measurement m;
    m.pm1_0_tenths = to_tenths(pm1_0_.median_x2(), 10);
    m.pm2_5_tenths = to_tenths(pm2_5_.median_x2(), 10);
    m.pm4_0_tenths = to_tenths(pm4_0_.median_x2(), 10);
    m.pm10_tenths = to_tenths(pm10_.median_x2(), 10);
    m.temperature_tenths = to_tenths(temperature_.median_x2(), 200);
    m.humidity_tenths = to_tenths(humidity_.median_x2(), 100);
    m.voc_index = to_integer(voc_.median_x2(), 10);
    m.nox_index = to_integer(nox_.median_x2(), 10);
    m.co2 = to_integer(co2_.median_x2(), 1);
    return m;
}

}  // namespace aq
