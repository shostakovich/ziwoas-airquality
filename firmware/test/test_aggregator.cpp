#include <doctest/doctest.h>

#include <vector>

#include "aggregator.h"
#include "backoff.h"

using aq::RawSample;
using aq::WindowAggregator;

namespace {

RawSample unknown_sample() {
    return RawSample{aq::kUnknownU16, aq::kUnknownU16, aq::kUnknownU16, aq::kUnknownU16, aq::kUnknownI16,
                     aq::kUnknownI16, aq::kUnknownI16, aq::kUnknownI16, aq::kUnknownU16};
}

RawSample sample(uint16_t pm, int16_t humidity, int16_t temperature, int16_t voc, int16_t nox, uint16_t co2) {
    return RawSample{pm, pm, pm, pm, humidity, temperature, voc, nox, co2};
}

std::optional<int64_t> median_of(std::vector<int32_t> v) { return aq::median_x2(v.data(), v.size()); }

}  // namespace

TEST_CASE("div_round_half_away rounds halves away from zero") {
    CHECK(aq::div_round_half_away(0, 10) == 0);
    CHECK(aq::div_round_half_away(14, 10) == 1);
    CHECK(aq::div_round_half_away(15, 10) == 2);
    CHECK(aq::div_round_half_away(25, 10) == 3);  // not banker's rounding
    CHECK(aq::div_round_half_away(-14, 10) == -1);
    CHECK(aq::div_round_half_away(-15, 10) == -2);
    CHECK(aq::div_round_half_away(-25, 10) == -3);
    CHECK(aq::div_round_half_away(-4, 10) == 0);
    CHECK(aq::div_round_half_away(7, 2) == 4);
    CHECK(aq::div_round_half_away(-7, 2) == -4);
}

TEST_CASE("median_x2") {
    CHECK_FALSE(median_of({}).has_value());
    CHECK(*median_of({5}) == 10);
    CHECK(*median_of({3, 1, 2}) == 4);        // odd: middle value 2
    CHECK(*median_of({4, 1, 3, 2}) == 5);     // even: (2 + 3) / 2 = 2.5
    CHECK(*median_of({-10, 10}) == 0);
    CHECK(*median_of({7, 7, 7, 1, 100}) == 14);
}

TEST_CASE("empty window yields all null") {
    WindowAggregator agg;
    agg.reset();
    CHECK(agg.sample_count() == 0);
    const aq::Measurement m = agg.result();
    CHECK_FALSE(m.pm1_0_tenths);
    CHECK_FALSE(m.temperature_tenths);
    CHECK_FALSE(m.co2);
}

TEST_CASE("all-unknown window yields null, counted as samples") {
    WindowAggregator agg;
    for (int i = 0; i < 5; ++i) agg.add(unknown_sample());
    CHECK(agg.sample_count() == 5);
    const aq::Measurement m = agg.result();
    CHECK_FALSE(m.pm1_0_tenths);
    CHECK_FALSE(m.pm2_5_tenths);
    CHECK_FALSE(m.pm4_0_tenths);
    CHECK_FALSE(m.pm10_tenths);
    CHECK_FALSE(m.temperature_tenths);
    CHECK_FALSE(m.humidity_tenths);
    CHECK_FALSE(m.voc_index);
    CHECK_FALSE(m.nox_index);
    CHECK_FALSE(m.co2);
}

TEST_CASE("odd count uses middle value and scales each field") {
    WindowAggregator agg;
    agg.add(sample(21, 4820, 4340, 1020, 10, 612));
    agg.add(sample(34, 4830, 4350, 1030, 20, 615));
    agg.add(sample(10, 4810, 4330, 1010, 10, 600));
    const aq::Measurement m = agg.result();
    CHECK(*m.pm1_0_tenths == 21);          // 2.1 ug/m3
    CHECK(*m.humidity_tenths == 482);      // 48.20 -> 48.2
    CHECK(*m.temperature_tenths == 217);   // 4340/200 = 21.70
    CHECK(*m.voc_index == 102);
    CHECK(*m.nox_index == 1);
    CHECK(*m.co2 == 612);
}

TEST_CASE("even count averages the two middle values") {
    WindowAggregator agg;
    agg.add(sample(20, 4800, 4300, 1000, 10, 600));
    agg.add(sample(21, 4815, 4350, 1015, 15, 601));
    const aq::Measurement m = agg.result();
    CHECK(*m.pm1_0_tenths == 21);          // 2.05 -> 2.1
    CHECK(*m.humidity_tenths == 481);      // 48.075 -> 48.1
    CHECK(*m.temperature_tenths == 216);   // 4325/200 = 21.625 -> 21.6
    CHECK(*m.voc_index == 101);            // 100.75 -> 101
    CHECK(*m.nox_index == 1);              // 1.25 -> 1
    CHECK(*m.co2 == 601);                  // 600.5 -> 601
}

TEST_CASE("unknown values are ignored per field") {
    WindowAggregator agg;
    RawSample a = unknown_sample();
    a.co2 = 500;
    RawSample b = unknown_sample();
    b.co2 = 700;
    b.nox_index = 30;
    RawSample c = unknown_sample();  // entirely unknown, must not pull the median
    agg.add(a);
    agg.add(b);
    agg.add(c);
    const aq::Measurement m = agg.result();
    CHECK(*m.co2 == 600);
    CHECK(*m.nox_index == 3);
    CHECK_FALSE(m.voc_index);
    CHECK_FALSE(m.pm2_5_tenths);
}

TEST_CASE("gas index 0 during start-up blackout counts as unknown") {
    WindowAggregator agg;
    RawSample a = unknown_sample();
    a.voc_index = 0;
    a.nox_index = 0;
    RawSample b = unknown_sample();
    b.voc_index = 0;
    b.nox_index = 10;
    RawSample c = unknown_sample();
    c.voc_index = 0;
    agg.add(a);
    agg.add(b);
    agg.add(c);
    const aq::Measurement m = agg.result();
    CHECK_FALSE(m.voc_index);
    CHECK(*m.nox_index == 1);
}

TEST_CASE("sentinels are exact: neighbours of the placeholders are real values") {
    WindowAggregator agg;
    RawSample s = unknown_sample();
    s.pm1_0 = 0xFFFE;
    s.humidity = 0x7FFE;
    s.co2 = 0;
    agg.add(s);
    const aq::Measurement m = agg.result();
    CHECK(*m.pm1_0_tenths == 65534);       // 6553.4 ug/m3
    CHECK(*m.humidity_tenths == 3277);     // 32766/100 = 327.66 -> 327.7
    CHECK(*m.co2 == 0);
}

TEST_CASE("rounding of negative temperatures and .x5 cases") {
    auto temp_tenths = [](std::vector<int16_t> raws) {
        WindowAggregator agg;
        for (int16_t r : raws) {
            RawSample s = unknown_sample();
            s.temperature = r;
            agg.add(s);
        }
        return *agg.result().temperature_tenths;
    };
    CHECK(temp_tenths({4350}) == 218);     // 21.75 -> 21.8
    CHECK(temp_tenths({-4350}) == -218);   // -21.75 -> -21.8
    CHECK(temp_tenths({-4340}) == -217);   // -21.70
    CHECK(temp_tenths({-10}) == -1);       // -0.05 -> -0.1
    CHECK(temp_tenths({-9}) == 0);         // -0.045 -> -0.0 -> 0
    CHECK(temp_tenths({-1, -2}) == 0);     // -0.0075 -> 0
    CHECK(temp_tenths({-20, 0}) == -1);    // median -10 -> -0.05 -> -0.1

    auto voc = [](std::vector<int16_t> raws) {
        WindowAggregator agg;
        for (int16_t r : raws) {
            RawSample s = unknown_sample();
            s.voc_index = r;
            agg.add(s);
        }
        return *agg.result().voc_index;
    };
    CHECK(voc({15}) == 2);      // 1.5 -> 2
    CHECK(voc({14}) == 1);
    CHECK(voc({25}) == 3);      // 2.5 -> 3 (half away from zero)
    CHECK(voc({10, 11}) == 1);  // 1.05 -> 1
    CHECK(voc({14, 15}) == 1);  // 1.45 -> 1
}

TEST_CASE("window of 60 samples") {
    WindowAggregator agg;
    // pm values 1..60 in scrambled order; median = (30 + 31) / 2 = 30.5 raw -> 3.05 -> 3.1
    for (int i = 0; i < 60; ++i) {
        const auto v = static_cast<uint16_t>((i * 37) % 60 + 1);
        RawSample s = sample(v, static_cast<int16_t>(5000 + v), static_cast<int16_t>(4000 + v),
                             static_cast<int16_t>(1000 + v), 10, static_cast<uint16_t>(400 + v));
        if (i % 10 == 0) s.co2 = aq::kUnknownU16;  // 6 unknown CO2 readings
        agg.add(s);
    }
    CHECK(agg.sample_count() == 60);
    const aq::Measurement m = agg.result();
    CHECK(*m.pm2_5_tenths == 31);
    CHECK(*m.humidity_tenths == 503);      // 5030.5/100 = 50.305 -> 50.3
    CHECK(*m.temperature_tenths == 202);   // 4030.5/200 = 20.1525 -> 20.2
    CHECK(*m.voc_index == 103);            // 1030.5/10 = 103.05 -> 103
    CHECK(*m.nox_index == 1);
    // CO2 drops 1,11,21,31,41,51; the middle pair of the remaining 54 is 30 and 32.
    CHECK(*m.co2 == 431);
}

TEST_CASE("reset clears the window and extra samples beyond capacity are dropped") {
    WindowAggregator agg;
    for (size_t i = 0; i < WindowAggregator::kCapacity + 10; ++i) agg.add(sample(10, 0, 0, 10, 10, 400));
    CHECK(agg.sample_count() == WindowAggregator::kCapacity);
    agg.reset();
    CHECK(agg.sample_count() == 0);
    CHECK_FALSE(agg.result().co2);
    agg.add(sample(10, 0, 0, 10, 10, 400));
    CHECK(*agg.result().co2 == 400);
}

TEST_CASE("backoff doubles up to 60 s and resets") {
    aq::Backoff b;
    const uint32_t expected[] = {1000, 2000, 4000, 8000, 16000, 32000, 60000, 60000};
    for (uint32_t e : expected) CHECK(b.next() == e);
    b.reset();
    CHECK(b.next() == 1000);
}

TEST_CASE("elapsed is wrap-safe") {
    CHECK(aq::elapsed(1000, 0, 1000));
    CHECK_FALSE(aq::elapsed(999, 0, 1000));
    CHECK(aq::elapsed(500, 0xFFFFFE00u, 1000));
    CHECK_FALSE(aq::elapsed(100, 0xFFFFFE00u, 1000));
}
