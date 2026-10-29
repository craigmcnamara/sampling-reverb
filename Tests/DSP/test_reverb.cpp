#include "FDNReverb.hpp"
#include "Signals.hpp"
#include "TestHarness.hpp"

using namespace sr;

namespace {

constexpr float kSR = 48000.f;

struct ReverbRig {
    FDNReverb reverb;
    sig::Buffer outL, outR;

    explicit ReverbRig(const FDNReverb::Params& p) {
        reverb.prepare(kSR);
        reverb.setParams(p);
    }
    void run(const sig::Buffer& inL, const sig::Buffer& inR) {
        const size_t base = outL.size();
        outL.resize(base + inL.size());
        outR.resize(base + inL.size());
        for (size_t i = 0; i < inL.size(); i += 256) {
            const int n = static_cast<int>(std::min<size_t>(256, inL.size() - i));
            reverb.process(&inL[i], &inR[i], &outL[base + i], &outR[base + i], n);
        }
    }
    void run(const sig::Buffer& mono) { run(mono, mono); }
};

FDNReverb::Params defaults() {
    FDNReverb::Params p;
    p.decaySec = 2.f;
    p.size = 0.5f;
    p.tone = 1.f;
    p.texture = 0.5f;
    p.mod = 0.f;
    return p;
}

} // namespace

TEST_CASE(pitch_shifter_shifts_up_an_octave) {
    PitchShifter ps;
    ps.prepare(kSR);
    ps.setSemitones(12.f);
    auto x = sig::sine(220.f, 1.f, 0.5f, kSR);
    sig::Buffer y(x.size());
    for (size_t i = 0; i < x.size(); ++i) y[i] = ps.process(x[i]);
    CHECK_NEAR(sig::zeroCrossingHz(y, 4800, y.size(), kSR), 440.f, 15.f);
}

TEST_CASE(pitch_shifter_shifts_down_a_fifth) {
    PitchShifter ps;
    ps.prepare(kSR);
    ps.setSemitones(-5.f);
    auto x = sig::sine(440.f, 1.f, 0.5f, kSR);
    sig::Buffer y(x.size());
    for (size_t i = 0; i < x.size(); ++i) y[i] = ps.process(x[i]);
    CHECK_NEAR(sig::zeroCrossingHz(y, 4800, y.size(), kSR), 440.f * std::pow(2.f, -5.f / 12.f), 12.f);
}

TEST_CASE(reverb_impulse_produces_decorrelated_stereo_tail) {
    ReverbRig rig(defaults());
    auto impulse = sig::silence(1.f, kSR);
    impulse[0] = 1.f;
    rig.run(impulse);
    const size_t from = sig::frames(0.2f, kSR), to = sig::frames(0.4f, kSR);
    CHECK(sig::rms(rig.outL, from, to) > 1e-3f);
    CHECK(sig::rms(rig.outR, from, to) > 1e-3f);
    double diff = 0.0;
    for (size_t i = from; i < to; ++i) diff += std::fabs(rig.outL[i] - rig.outR[i]);
    CHECK(diff > 1.0);
}

TEST_CASE(reverb_decay_time_tracks_rt60) {
    for (float rt60 : {1.f, 4.f}) {
        auto p = defaults();
        p.decaySec = rt60;
        ReverbRig rig(p);
        auto noise = sig::noiseBurst(0.05f, 0.5f, kSR);
        noise.resize(sig::frames(3.f, kSR), 0.f);
        rig.run(noise);
        // Expected level drop between two windows 0.5 s apart: 60 dB * 0.5 / RT60.
        const float a = sig::rms(rig.outL, sig::frames(0.3f, kSR), sig::frames(0.4f, kSR));
        const float b = sig::rms(rig.outL, sig::frames(0.8f, kSR), sig::frames(0.9f, kSR));
        const float expected = -60.f * 0.5f / rt60;
        CHECK_NEAR(sig::db(b) - sig::db(a), expected, std::fabs(expected) * 0.35f + 1.f);
    }
}

TEST_CASE(reverb_freeze_holds_the_tail_and_ignores_input) {
    auto p = defaults();
    p.decaySec = 30.f;
    ReverbRig rig(p);
    rig.run(sig::noiseBurst(0.5f, 0.5f, kSR));
    p.freeze = true;
    rig.reverb.setParams(p);
    rig.run(sig::noiseBurst(3.f, 0.5f, kSR, 7));  // input during freeze must be ignored
    const size_t base = sig::frames(0.5f, kSR);
    const float early = sig::rms(rig.outL, base + sig::frames(0.5f, kSR), base + sig::frames(1.f, kSR));
    const float late = sig::rms(rig.outL, base + sig::frames(2.5f, kSR), base + sig::frames(3.f, kSR));
    CHECK(early > 1e-3f);
    CHECK(sig::db(late) - sig::db(early) > -3.f);
    CHECK(sig::db(late) - sig::db(early) < 3.f);
}

TEST_CASE(reverb_freeze_from_silence_stays_silent) {
    auto p = defaults();
    p.freeze = true;
    ReverbRig rig(p);
    rig.run(sig::noiseBurst(0.5f, 0.5f, kSR));
    CHECK(sig::peak(rig.outL) == 0.f);
}

TEST_CASE(reverb_extreme_settings_stay_bounded) {
    auto p = defaults();
    p.decaySec = 30.f;
    p.size = 1.f;
    p.texture = 1.f;
    p.mod = 1.f;
    p.shimmer = 1.f;
    p.shimmerSemitones = 19.f;
    ReverbRig rig(p);
    sig::Buffer loud = sig::noiseBurst(10.f, 1.f, kSR, 3);
    rig.run(loud);
    CHECK(sig::allFinite(rig.outL));
    CHECK(sig::allFinite(rig.outR));
    CHECK(sig::peak(rig.outL) < 8.f);
}
