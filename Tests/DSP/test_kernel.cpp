#include "SamplingReverbKernel.hpp"
#include "Signals.hpp"
#include "TestHarness.hpp"

using namespace sr;

namespace {

constexpr float kSR = 48000.f;

struct KernelRig {
    Kernel kernel;
    sig::Buffer outL, outR;
    int block = 256;

    explicit KernelRig(float sampleRate = kSR) { kernel.prepare(sampleRate); }

    void run(const sig::Buffer& inL, const sig::Buffer& inR) {
        const size_t base = outL.size();
        outL.resize(base + inL.size());
        outR.resize(base + inL.size());
        for (size_t i = 0; i < inL.size(); i += block) {
            const int n = static_cast<int>(std::min<size_t>(block, inL.size() - i));
            kernel.process(&inL[i], &inR[i], &outL[base + i], &outR[base + i], n);
        }
    }
    void run(const sig::Buffer& mono) { run(mono, mono); }
    void set(SRParam p, float v) { kernel.setParameter(p, v); }
};

} // namespace

TEST_CASE(kernel_defaults_match_param_specs) {
    Kernel k;
    for (const auto& spec : kParamSpecs) CHECK(k.getParameter(spec.address) == spec.def);
}

TEST_CASE(kernel_param_specs_are_ordered_by_address) {
    for (uint64_t i = 0; i < SRParamCount; ++i) CHECK(kParamSpecs[i].address == static_cast<SRParam>(i));
}

TEST_CASE(kernel_parameter_roundtrip_and_bad_addresses_are_ignored) {
    Kernel k;
    k.setParameter(SRParamMix, 12.f);
    CHECK(k.getParameter(SRParamMix) == 12.f);
    k.setParameter(SRParamCount + 5, 1.f);
    CHECK(k.getParameter(SRParamCount + 5) == 0.f);
}

TEST_CASE(kernel_bypass_passes_input_through_exactly) {
    KernelRig rig;
    rig.kernel.setBypass(true);
    auto in = sig::noiseBurst(0.5f, 0.5f, kSR);
    rig.run(in);
    for (size_t i = 0; i < in.size(); ++i) REQUIRE(rig.outL[i] == in[i] && rig.outR[i] == in[i]);
}

TEST_CASE(kernel_mix_zero_is_dry) {
    KernelRig rig;
    rig.set(SRParamMix, 0.f);
    auto in = sig::noiseBurst(0.5f, 0.5f, kSR);
    rig.run(in);
    for (size_t i = 0; i < in.size(); ++i) REQUIRE(std::fabs(rig.outL[i] - in[i]) < 1e-6f);
}

TEST_CASE(kernel_wet_only_hears_just_samples_when_dry_feed_is_zero) {
    KernelRig rig;
    rig.set(SRParamMix, 100.f);
    rig.set(SRParamDryFeed, 0.f);
    rig.run(sig::toneBurst(220.f, 0.3f, 0.5f, kSR));  // captured, never matched
    rig.run(sig::silence(1.f, kSR));
    CHECK(sig::peak(rig.outL) < 1e-6f);
    CHECK(sig::peak(rig.outR) < 1e-6f);
}

TEST_CASE(kernel_matched_sound_plays_sample_into_reverb) {
    KernelRig rig;
    rig.set(SRParamMix, 100.f);
    rig.set(SRParamDryFeed, 0.f);
    rig.run(sig::toneBurst(220.f, 0.3f, 0.5f, kSR));
    rig.run(sig::silence(1.f, kSR));
    const size_t before = rig.outL.size();
    rig.run(sig::toneBurst(220.f, 0.3f, 0.5f, kSR));
    rig.run(sig::silence(1.f, kSR));
    CHECK(sig::rms(rig.outL, before, rig.outL.size()) > 1e-3f);
    CHECK(rig.kernel.engine().totalTriggers() == 1);
}

TEST_CASE(kernel_dry_feed_sends_live_input_to_the_reverb) {
    KernelRig rig;
    rig.set(SRParamMix, 100.f);
    rig.set(SRParamDryFeed, 100.f);
    auto impulse = sig::silence(1.f, kSR);
    impulse[0] = 1.f;
    rig.run(impulse);
    CHECK(sig::rms(rig.outL, sig::frames(0.2f, kSR), sig::frames(0.5f, kSR)) > 1e-4f);
}

TEST_CASE(kernel_in_place_matches_out_of_place) {
    KernelRig a, b;
    auto in = sig::toneBurst(330.f, 1.f, 0.5f, kSR);
    a.run(in);
    sig::Buffer l = in, r = in;
    for (size_t i = 0; i < l.size(); i += 256) {
        const int n = static_cast<int>(std::min<size_t>(256, l.size() - i));
        b.kernel.process(&l[i], &r[i], &l[i], &r[i], n);
    }
    for (size_t i = 0; i < l.size(); ++i) REQUIRE(l[i] == a.outL[i] && r[i] == a.outR[i]);
}

TEST_CASE(kernel_handles_odd_and_large_block_sizes) {
    for (int block : {1, 37, 255, 257, 4096}) {
        KernelRig rig;
        rig.block = block;
        sig::Buffer in = sig::toneBurst(220.f, 0.4f, 0.5f, kSR);
        sig::append(in, sig::silence(1.f, kSR));
        rig.run(in);
        CHECK(sig::allFinite(rig.outL));
        CHECK(rig.kernel.engine().poolCount() == 1);
    }
}

TEST_CASE(kernel_publishes_visual_state) {
    KernelRig rig;
    rig.run(sig::toneBurst(220.f, 0.3f, 0.5f, kSR));
    rig.run(sig::silence(1.f, kSR));

    std::vector<SRSlotVisual> slots(SR_MAX_SLOTS);
    std::vector<float> scopeL(SR_SCOPE_POINTS), scopeR(SR_SCOPE_POINTS);
    VisualSummary summary = rig.kernel.readVisual(slots.data(), SR_MAX_SLOTS, scopeL.data(), scopeR.data(), SR_SCOPE_POINTS);
    CHECK(summary.poolCount == 1);
    CHECK(!summary.frozen);
    int active = 0;
    for (const auto& s : slots) active += s.active;
    CHECK(active == 1);
    CHECK(sig::peak(scopeL) > 0.f);  // reverb tail of the dry feed

    rig.set(SRParamFreeze, 1.f);
    rig.run(sig::silence(0.1f, kSR));
    summary = rig.kernel.readVisual(slots.data(), SR_MAX_SLOTS, nullptr, nullptr, 0);
    CHECK(summary.frozen);
}

TEST_CASE(kernel_maps_parameters_onto_the_engine) {
    KernelRig rig;
    rig.set(SRParamRetention, 0.f);  // 3 s lifetime
    rig.run(sig::toneBurst(220.f, 0.3f, 0.5f, kSR));
    rig.run(sig::silence(1.f, kSR));
    CHECK(rig.kernel.engine().poolCount() == 1);
    rig.run(sig::silence(3.5f, kSR));
    CHECK(rig.kernel.engine().poolCount() == 0);
}

TEST_CASE(kernel_supports_common_sample_rates) {
    for (float rate : {44100.f, 48000.f, 88200.f, 96000.f, 192000.f}) {
        KernelRig rig(rate);
        sig::Buffer in = sig::toneBurst(220.f, 0.3f, 0.5f, rate);
        sig::append(in, sig::silence(1.f, rate));
        rig.run(in);
        CHECK(sig::allFinite(rig.outL));
        CHECK(rig.kernel.engine().poolCount() == 1);
        CHECK_NEAR(rig.kernel.engine().lastFeatures().midi, 57.f, 0.35);
    }
}

TEST_CASE(kernel_random_parameter_abuse_stays_finite_and_bounded) {
    KernelRig rig;
    Rng rng;
    sig::Buffer in;
    for (int i = 0; i < 40; ++i) {
        sig::append(in, i % 2 ? sig::noiseBurst(0.25f, 1.f, kSR, i + 1) : sig::toneBurst(110.f * (1 + i % 5), 0.25f, 1.f, kSR));
    }
    for (size_t i = 0; i < in.size(); i += 4096) {
        for (const auto& spec : kParamSpecs) rig.set(spec.address, lerpf(spec.min, spec.max, rng.uniform()));
        rig.set(SRParamFreeze, 0.f);
        const size_t n = std::min<size_t>(4096, in.size() - i);
        rig.run(sig::Buffer(in.begin() + i, in.begin() + i + n));
    }
    CHECK(sig::allFinite(rig.outL));
    CHECK(sig::allFinite(rig.outR));
    CHECK(sig::peak(rig.outL) < 8.f);
}

TEST_CASE(kernel_turning_up_shimmer_keeps_the_reverb_audible) {
    for (float shimmer : {25.f, 50.f, 100.f}) {
        KernelRig rig;
        rig.set(SRParamMix, 100.f);  // wet only, so "goes dry" shows up as silence
        sig::Buffer phrase;
        for (int i = 0; i < 4; ++i) {
            sig::append(phrase, sig::toneBurst(i % 2 ? 330.f : 220.f, 0.25f, 0.5f, kSR));
            sig::append(phrase, sig::silence(0.25f, kSR));
        }
        rig.run(phrase);  // 2 s with shimmer off
        rig.set(SRParamShimmer, shimmer);
        for (int i = 0; i < 5; ++i) rig.run(phrase);  // 10 s with shimmer on

        bool ok = sig::allFinite(rig.outL) && sig::allFinite(rig.outR);
        for (float t = 2.f; t < 12.f && ok; t += 0.5f) {
            const float level = sig::rms(rig.outL, sig::frames(t, kSR), sig::frames(t + 0.5f, kSR));
            if (!(level > 1e-3f)) {
                std::printf("    shimmer %g%%: wet level %g at %.1f s\n", shimmer, level, t);
                ok = false;
            }
        }
        CHECK(ok);
        CHECK(sig::peak(rig.outL) < 4.f);
    }
}

namespace {

// Plays a phrase of notes into the kernel (wet only) and returns per-channel wet RMS over the last 4 s.
std::pair<float, float> lateWetLevels(float shimmer, int interval, bool leftOnly, float sub = 0.f) {
    KernelRig rig;
    rig.set(SRParamMix, 100.f);
    rig.set(SRParamSub, sub);
    rig.set(SRParamShimmer, shimmer);
    rig.set(SRParamInterval, static_cast<float>(interval));
    sig::Buffer l;
    for (int i = 0; i < 20; ++i) {
        sig::append(l, sig::toneBurst(i % 2 ? 330.f : 220.f, 0.25f, 0.5f, kSR));
        sig::append(l, sig::silence(0.25f, kSR));
    }
    const sig::Buffer r = leftOnly ? sig::Buffer(l.size(), 0.f) : l;
    rig.run(l, r);
    const size_t from = l.size() - sig::frames(4.f, kSR);
    return {sig::rms(rig.outL, from, l.size()), sig::rms(rig.outR, from, l.size())};
}

} // namespace

TEST_CASE(kernel_shimmer_never_runs_away) {
    for (bool leftOnly : {false, true}) {
        const auto base = lateWetLevels(0.f, 4, leftOnly);
        for (int interval = 0; interval < kIntervalCount; ++interval) {
            const auto shim = lateWetLevels(100.f, interval, leftOnly);
            const float gainL = sig::db(shim.first) - sig::db(base.first);
            const float gainR = sig::db(shim.second) - sig::db(base.second);
            if (gainL > 6.f || gainR > 6.f)
                std::printf("    %s input, interval %s: shimmer raised wet by L %+.1f dB, R %+.1f dB\n",
                            leftOnly ? "left-only" : "stereo", kIntervalNames[interval], gainL, gainR);
            CHECK(gainL <= 6.f);
            CHECK(gainR <= 6.f);
        }
    }
}

TEST_CASE(kernel_left_only_input_gives_balanced_wet) {
    for (float shimmer : {0.f, 100.f}) {
        const auto w = lateWetLevels(shimmer, 4, true);
        const float balance = sig::db(w.first) - sig::db(w.second);
        if (std::fabs(balance) > 6.f) std::printf("    shimmer %g%%: L-R wet balance %+.1f dB\n", shimmer, balance);
        CHECK(std::fabs(balance) <= 6.f);
    }
}

TEST_CASE(kernel_shimmer_adds_energy_at_the_shifted_pitch) {
    // Hann-windowed DFT energy summed over [lo, hi] Hz (1 Hz steps) over the tail of the wet output.
    // The band is wide because a grain pitch shifter spreads its output into sidebands around the
    // target pitch.
    auto bandEnergy = [](const sig::Buffer& x, size_t from, float lo, float hi) {
        const size_t n = x.size() - from;
        double total = 0.0;
        for (float f = lo; f <= hi; f += 1.f) {
            double re = 0.0, im = 0.0;
            const double w = 2.0 * 3.14159265358979 * f / kSR;
            for (size_t i = 0; i < n; ++i) {
                const double hann = 0.5 - 0.5 * std::cos(2.0 * 3.14159265358979 * i / n);
                re += x[from + i] * hann * std::cos(w * i);
                im -= x[from + i] * hann * std::sin(w * i);
            }
            total += re * re + im * im;
        }
        return total;
    };
    // Level of the shifted pitch relative to the 220 Hz fundamental, in dB.
    auto shiftedVsFundamental = [&](float shimmer, int interval, float shiftedHz, float sub = 0.f) {
        KernelRig rig;
        rig.set(SRParamMix, 100.f);
        rig.set(SRParamDecay, 8.f);
        rig.set(SRParamMod, 0.f);
        rig.set(SRParamSub, sub);
        rig.set(SRParamShimmer, shimmer);
        rig.set(SRParamInterval, static_cast<float>(interval));
        rig.run(sig::sine(220.f, 4.f, 0.3f, kSR));
        const size_t from = sig::frames(3.f, kSR);
        const double shifted = bandEnergy(rig.outL, from, shiftedHz - 40.f, shiftedHz + 40.f);
        const double fundamental = bandEnergy(rig.outL, from, 205.f, 235.f);
        return 10.f * std::log10(static_cast<float>(shifted / fundamental));
    };
    const int octaveUp = 4, octaveDown = 0;  // indexes into kIntervalSemitones
    const float up = shiftedVsFundamental(100.f, octaveUp, 440.f);
    const float down = shiftedVsFundamental(100.f, octaveDown, 110.f);
    const float off = shiftedVsFundamental(0.f, octaveUp, 440.f);
    std::printf("    shifted pitch vs fundamental: +12 %.1f dB, -12 %.1f dB, shimmer off %.1f dB\n", up, down, off);
    CHECK(up > -12.f);
    CHECK(down > -12.f);
    CHECK(off < -25.f);

    // Sub: a fixed octave down, independent of Shimmer and Interval.
    const float sub = shiftedVsFundamental(0.f, octaveUp, 110.f, 100.f);
    const float subOff = shiftedVsFundamental(0.f, octaveUp, 110.f, 0.f);
    const float subWithShimmerUp = shiftedVsFundamental(0.f, octaveUp, 440.f, 100.f);
    std::printf("    sub: 110 Hz vs fundamental %.1f dB (off %.1f dB); 440 Hz with sub only %.1f dB\n", sub, subOff, subWithShimmerUp);
    CHECK(sub > -12.f);
    CHECK(subOff < -25.f);
    CHECK(subWithShimmerUp < -25.f);  // Sub doesn't add upward shimmer
}

TEST_CASE(kernel_sub_never_runs_away) {
    for (bool leftOnly : {false, true}) {
        const auto base = lateWetLevels(0.f, 4, leftOnly);
        for (float shimmer : {0.f, 100.f}) {
            const auto sub = lateWetLevels(shimmer, 4, leftOnly, 100.f);
            const float gainL = sig::db(sub.first) - sig::db(base.first);
            const float gainR = sig::db(sub.second) - sig::db(base.second);
            if (gainL > 6.f || gainR > 6.f)
                std::printf("    %s input, shimmer %g%%: sub raised wet by L %+.1f dB, R %+.1f dB\n",
                            leftOnly ? "left-only" : "stereo", shimmer, gainL, gainR);
            CHECK(gainL <= 6.f);
            CHECK(gainR <= 6.f);
        }
    }
}
