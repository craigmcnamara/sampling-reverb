#include "SamplingReverbKernel.hpp"

#include <array>
#include "Signals.hpp"
#include "TestHarness.hpp"

using namespace sr;

namespace {

constexpr float kSR = 48000.f;

struct KernelRig {
    Kernel kernel;
    sig::Buffer outL, outR;
    int block = 256;

    explicit KernelRig(float sampleRate = kSR) {
        kernel.prepare(sampleRate);
        set(SRParamLength, 0.8f);  // short captures keep the tests quick; the default is longer
    }

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

TEST_CASE(kernel_attack_parameter_reaches_the_voices) {
    // Wet-only, samples only: compare how loud the replayed sample is in its first 30 ms.
    auto onset = [](float attackMs) {
        KernelRig rig;
        rig.set(SRParamMix, 100.f);
        rig.set(SRParamDryFeed, 0.f);
        rig.set(SRParamDecay, 0.3f);
        rig.set(SRParamAttack, attackMs);
        rig.run(sig::toneBurst(220.f, 0.4f, 0.5f, kSR));
        rig.run(sig::silence(1.f, kSR));
        const size_t start = rig.outL.size();
        rig.run(sig::toneBurst(220.f, 0.4f, 0.5f, kSR));
        size_t voice = start;
        while (voice < rig.outL.size() && std::fabs(rig.outL[voice]) < 1e-7f) ++voice;
        return sig::rms(rig.outL, voice, voice + sig::frames(0.03f, kSR));
    };
    const float fast = onset(1.f), slow = onset(1000.f);
    CHECK(fast > 1e-3f);
    CHECK(slow < 0.25f * fast);
}

TEST_CASE(kernel_length_goes_to_twenty_seconds) {
    const ParamSpec& length = kParamSpecs[SRParamLength];
    CHECK(length.max == 20.f);
    CHECK(length.exponential);  // short lengths stay easy to dial in
    CHECK(length.def >= 2.f);
}

namespace {

// Wet output of a few strummed guitar chords followed by the tail.
sig::Buffer wetGuitar(std::initializer_list<std::pair<SRParam, float>> params) {
    KernelRig rig;
    rig.set(SRParamMix, 100.f);
    rig.set(SRParamDecay, 8.f);
    for (const auto& [p, v] : params) rig.set(p, v);
    sig::Buffer in;
    for (int i = 0; i < 4; ++i) sig::append(in, sig::strum(2.f, 0.5f, kSR));
    sig::append(in, sig::silence(6.f, kSR));
    rig.run(in);
    return rig.outL;
}

} // namespace

TEST_CASE(kernel_pitch_layers_keep_the_reverbs_body) {
    // Shimmer and Sub should add layers, not hollow out the guitar range (125 Hz – 1 kHz).
    const size_t from = sig::frames(4.f, kSR);
    const float plain = sig::bandEnergyDb(wetGuitar({}), from, 125.f, 1000.f, kSR);
    const float shimmer = sig::bandEnergyDb(wetGuitar({{SRParamShimmer, 50.f}}), from, 125.f, 1000.f, kSR);
    const float sub = sig::bandEnergyDb(wetGuitar({{SRParamSub, 50.f}}), from, 125.f, 1000.f, kSR);
    std::printf("    body (125 Hz-1 kHz) change: shimmer 50%% %+.1f dB, sub 50%% %+.1f dB\n", shimmer - plain, sub - plain);
    CHECK(shimmer - plain > -3.f);
    CHECK(sub - plain > -3.f);
}

TEST_CASE(kernel_sub_adds_the_undertone_series) {
    // A held 330 Hz note with Sub up gains energy at f/2, f/3 and f/4.
    auto levels = [](float sub) {
        KernelRig rig;
        rig.set(SRParamMix, 100.f);
        rig.set(SRParamDecay, 8.f);
        rig.set(SRParamMod, 0.f);
        rig.set(SRParamSub, sub);
        rig.run(sig::sine(330.f, 4.f, 0.3f, kSR));
        const size_t from = sig::frames(2.f, kSR), to = rig.outL.size();
        const float f0 = sig::toneLevel(rig.outL, from, to, 330.f, kSR);
        std::array<float, 3> rel {};
        const float undertones[3] = {165.f, 110.f, 82.5f};
        for (int k = 0; k < 3; ++k) {
            // Grain shifters spread energy into sidebands, so take the strongest bin within ±12 Hz.
            float best = 0.f;
            for (float d = -12.f; d <= 12.f; d += 1.f)
                best = std::max(best, sig::toneLevel(rig.outL, from, to, undertones[k] + d, kSR));
            rel[k] = sig::db(best) - sig::db(f0);
        }
        return rel;
    };
    const auto on = levels(100.f), off = levels(0.f);
    std::printf("    undertones vs fundamental, sub on: f/2 %.1f, f/3 %.1f, f/4 %.1f dB (off: %.1f, %.1f, %.1f)\n",
                on[0], on[1], on[2], off[0], off[1], off[2]);
    for (int k = 0; k < 3; ++k) {
        CHECK(on[k] > -24.f);
        CHECK(on[k] - off[k] > 15.f);
    }
}

// ---- Tempo and quantize ----------------------------------------------------------------------

namespace {

// Runs `in` through the kernel in small blocks, telling it the host time before each block via
// `host(frame)`. Returns the frame at which the first new trigger fired (to within one block), or -1.
template <typename HostFn>
long long kernelTriggerFrame(Kernel& k, const sig::Buffer& in, long long& clock, HostFn host) {
    constexpr int kBlock = 16;
    const uint64_t before = k.engine().totalTriggers();
    long long fired = -1;
    float l[kBlock], r[kBlock];
    for (size_t i = 0; i < in.size(); i += kBlock, clock += kBlock) {
        const int n = static_cast<int>(std::min<size_t>(kBlock, in.size() - i));
        k.setHostTime(host(clock));
        k.process(&in[i], &in[i], l, r, n);
        if (fired < 0 && k.engine().totalTriggers() != before) fired = clock;
    }
    return fired;
}

// Captures a note, then replays it `offset` frames after second 3. Returns {trigger, onset}.
template <typename HostFn>
std::pair<long long, long long> quantizedReplay(Kernel& k, long long offset, HostFn host) {
    long long clock = 0;
    auto tone = sig::toneBurst(440.f, 0.3f, 0.5f, kSR);
    kernelTriggerFrame(k, tone, clock, host);
    kernelTriggerFrame(k, sig::silence(3.f - static_cast<float>(clock) / kSR + offset / kSR, kSR), clock, host);
    const long long onset = clock;
    sig::append(tone, sig::silence(2.f, kSR));  // long enough to reach a whole-bar grid point
    return {kernelTriggerFrame(k, tone, clock, host), onset};
}

} // namespace

TEST_CASE(kernel_quantize_follows_the_host_transport) {
    Kernel k;
    k.prepare(kSR);
    k.setParameter(SRParamLength, 0.5f);
    k.setParameter(SRParamQuantize, 1.f);
    k.setParameter(SRParamGrid, 5.f);    // 1/4
    k.setParameter(SRParamTempo, 77.f);  // ignored: the host has a tempo
    // The host plays at 100 BPM from beat 10.4, so quarter notes fall where beat is a whole number.
    const double bpf = 100.0 / (60.0 * kSR);
    auto host = [&](long long frame) {
        HostTime t;
        t.hasTempo = true;
        t.tempo = 100.0;
        t.hasPosition = true;
        t.beat = 10.4 + frame * bpf;
        t.downbeat = 8.0;
        t.hasTransport = true;
        t.playing = true;
        return t;
    };
    const auto [fired, onset] = quantizedReplay(k, 5000, host);
    REQUIRE(fired >= 0);
    const double beat = 10.4 + fired * bpf;
    CHECK(std::fabs(beat - std::round(beat)) < 16 * bpf);
    CHECK(fired > onset + 2048);
    CHECK(k.clock().fromHost);
    CHECK_NEAR(k.clock().tempo, 100.0, 1e-9);
}

TEST_CASE(kernel_quantize_runs_free_at_the_tempo_parameter_without_a_host) {
    Kernel k;
    k.prepare(kSR);
    k.setParameter(SRParamLength, 0.5f);
    k.setParameter(SRParamQuantize, 1.f);
    k.setParameter(SRParamGrid, 3.f);    // 1/8
    k.setParameter(SRParamTempo, 90.f);  // 1/8 = 16000 frames
    const auto [fired, onset] = quantizedReplay(k, 7000, [](long long) { return HostTime {}; });
    REQUIRE(fired >= 0);
    CHECK(fired % 16000 < 16 || fired % 16000 > 16000 - 16);
    CHECK(fired > onset + 2048);
    CHECK(!k.clock().fromHost);
    CHECK_NEAR(k.clock().tempo, 90.0, 1e-9);
}

TEST_CASE(kernel_stopped_transport_keeps_a_running_grid_at_host_tempo) {
    // Stopped, the host's beat stays put; the grid must keep running instead of stalling there.
    Kernel k;
    k.prepare(kSR);
    k.setParameter(SRParamLength, 0.5f);
    k.setParameter(SRParamQuantize, 1.f);
    k.setParameter(SRParamGrid, 3.f);
    auto host = [](long long) {
        HostTime t;
        t.hasTempo = true;
        t.tempo = 120.0;
        t.hasPosition = true;
        t.beat = 33.3;
        t.hasTransport = true;
        t.playing = false;
        return t;
    };
    const auto [fired, onset] = quantizedReplay(k, 2000, host);
    REQUIRE(fired >= 0);
    CHECK(fired > onset + 2048);
    CHECK(fired <= onset + 2048 + 12000 + 16);
}

TEST_CASE(kernel_grid_bar_uses_the_host_time_signature) {
    // 3/4 at 120 BPM: a bar is 3 beats = 72000 frames, counted from the downbeat at beat 0.
    Kernel k;
    k.prepare(kSR);
    k.setParameter(SRParamLength, 0.5f);
    k.setParameter(SRParamQuantize, 1.f);
    k.setParameter(SRParamGrid, 7.f);
    const double bpf = 120.0 / (60.0 * kSR);
    auto host = [&](long long frame) {
        HostTime t;
        t.hasTempo = true;
        t.tempo = 120.0;
        t.hasPosition = true;
        t.beat = frame * bpf;
        t.barBeats = 3.0;
        t.downbeat = std::floor(t.beat / 3.0) * 3.0;
        t.hasTransport = true;
        t.playing = true;
        return t;
    };
    const auto [fired, onset] = quantizedReplay(k, 1000, host);
    REQUIRE(fired >= 0);
    CHECK(fired % 72000 < 16 || fired % 72000 > 72000 - 16);
    CHECK(fired > onset + 2048);
}
