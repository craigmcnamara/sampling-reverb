#include "DSPUtil.hpp"
#include "Signals.hpp"
#include "TestHarness.hpp"
#include "TripleBuffer.hpp"

#include <atomic>
#include <thread>

using namespace sr;

TEST_CASE(dsputil_next_pow2) {
    CHECK(nextPow2(1) == 1);
    CHECK(nextPow2(3) == 4);
    CHECK(nextPow2(4) == 4);
    CHECK(nextPow2(1025) == 2048);
}

TEST_CASE(dsputil_safe_limit_is_transparent_below_unity_and_bounded) {
    CHECK(safeLimit(0.5f) == 0.5f);
    CHECK(safeLimit(-1.f) == -1.f);
    CHECK(safeLimit(1.5f) > 1.f);
    CHECK(safeLimit(1000.f) <= 2.f);
    CHECK(safeLimit(-1000.f) >= -2.f);
}

TEST_CASE(delayline_reads_integer_and_fractional_delays) {
    DelayLine d;
    d.allocate(16);
    for (int i = 1; i <= 8; ++i) d.write(static_cast<float>(i));
    CHECK(d.readInt(0) == 8.f);
    CHECK(d.readInt(3) == 5.f);
    CHECK_NEAR(d.read(0.f), 8.f, 1e-6);
    CHECK_NEAR(d.read(1.5f), 6.5f, 1e-6);
}

TEST_CASE(allpass_preserves_impulse_energy) {
    Allpass ap;
    ap.allocate(37);
    ap.g = 0.6f;
    double energy = 0.0;
    for (int i = 0; i < 20000; ++i) {
        const float y = ap.process(i == 0 ? 1.f : 0.f);
        energy += double(y) * y;
    }
    CHECK_NEAR(energy, 1.0, 1e-3);
}

TEST_CASE(svf_lowpass_and_highpass_split_the_spectrum) {
    const float sr = 48000.f;
    auto measure = [&](float hz, bool high) {
        SVF f;
        f.set(1000.f, 0.707f, sr);
        auto x = sig::sine(hz, 0.5f, 1.f, sr);
        sig::Buffer y(x.size());
        for (size_t i = 0; i < x.size(); ++i) {
            float lp, hp;
            f.process(x[i], lp, hp);
            y[i] = high ? hp : lp;
        }
        return sig::rms(y, y.size() / 2, y.size()) / sig::rms(x, x.size() / 2, x.size());
    };
    CHECK(measure(100.f, false) > 0.95f);
    CHECK(measure(10000.f, false) < 0.02f);
    CHECK(measure(10000.f, true) > 0.95f);
    CHECK(measure(100.f, true) < 0.02f);
}

TEST_CASE(triple_buffer_reader_sees_latest_publish_only) {
    TripleBuffer<int> tb;
    CHECK(tb.read() == 0);
    tb.backBuffer() = 1;
    CHECK(tb.read() == 0);  // not yet published
    tb.publish();
    tb.backBuffer() = 2;
    tb.publish();
    tb.backBuffer() = 3;
    tb.publish();
    CHECK(tb.read() == 3);
    CHECK(tb.read() == 3);  // stable without new publishes
}

TEST_CASE(triple_buffer_never_tears_under_concurrency) {
    struct Pair { int a = 0, b = 0; };
    TripleBuffer<Pair> tb;
    std::atomic<bool> done {false};
    std::thread producer([&] {
        for (int i = 1; i <= 200000; ++i) {
            tb.backBuffer() = {i, -i};
            tb.publish();
        }
        done = true;
    });
    int torn = 0, last = 0, regressions = 0;
    while (!done) {
        const Pair p = tb.read();
        if (p.a != -p.b) ++torn;
        if (p.a < last) ++regressions;
        last = p.a;
    }
    producer.join();
    CHECK(torn == 0);
    CHECK(regressions == 0);
    CHECK(tb.read().a == 200000);
}
