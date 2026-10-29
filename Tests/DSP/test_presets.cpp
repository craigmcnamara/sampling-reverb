#include "Presets.hpp"
#include "SamplingReverbKernel.hpp"
#include "Signals.hpp"
#include "TestHarness.hpp"

#include <cmath>
#include <cstring>

using namespace sr;

TEST_CASE(presets_there_are_seven_with_unique_names) {
    CHECK(kPresetCount == 7);
    for (int i = 0; i < kPresetCount; ++i) {
        REQUIRE(kPresets[i].name != nullptr);
        CHECK(std::strlen(kPresets[i].name) > 0);
        CHECK(std::strlen(kPresets[i].name) <= 16);  // must fit a toolbar button
        for (int j = i + 1; j < kPresetCount; ++j) CHECK(std::strcmp(kPresets[i].name, kPresets[j].name) != 0);
    }
}

TEST_CASE(presets_values_are_within_parameter_ranges) {
    for (const auto& preset : kPresets) {
        for (const auto& spec : kParamSpecs) {
            const float v = preset.values[spec.address];
            if (!(v >= spec.min && v <= spec.max)) {
                std::printf("    %s: %s = %g outside [%g, %g]\n", preset.name, spec.identifier, v, spec.min, spec.max);
                CHECK(false);
            }
            if (spec.unit == Unit::Indexed || spec.unit == Unit::Boolean || spec.address == SRParamDensity)
                CHECK(v == std::round(v));
        }
    }
}

TEST_CASE(presets_never_start_frozen) {
    for (const auto& preset : kPresets) CHECK(preset.values[SRParamFreeze] == 0.f);
}

TEST_CASE(presets_are_distinct_from_each_other) {
    for (int i = 0; i < kPresetCount; ++i)
        for (int j = i + 1; j < kPresetCount; ++j) {
            int differing = 0;
            for (uint64_t p = 0; p < SRParamCount; ++p) differing += kPresets[i].values[p] != kPresets[j].values[p];
            CHECK(differing >= 5);
        }
}

TEST_CASE(presets_all_sound_stable_and_audible) {
    constexpr float kSR = 48000.f;
    sig::Buffer phrase;
    for (int i = 0; i < 6; ++i) {
        sig::append(phrase, i % 3 == 2 ? sig::noiseBurst(0.3f, 0.5f, kSR, i + 1)
                                       : sig::toneBurst(i % 2 ? 330.f : 220.f, 0.3f, 0.5f, kSR));
        sig::append(phrase, sig::silence(0.5f, kSR));
    }
    // Play, wait out the longest Length so every capture lands, then play again: presets with
    // Dry Feed at 0 only make sound once samples are replayed.
    float longest = 0.f;
    for (const auto& preset : kPresets) longest = std::max(longest, preset.values[SRParamLength]);
    sig::Buffer in = phrase;
    sig::append(in, sig::silence(longest + 1.f, kSR));
    sig::append(in, phrase);
    sig::append(in, sig::silence(2.f, kSR));

    for (const auto& preset : kPresets) {
        Kernel k;
        for (uint64_t p = 0; p < SRParamCount; ++p) k.setParameter(p, preset.values[p]);
        k.prepare(kSR);
        sig::Buffer l(in.size()), r(in.size());
        for (size_t i = 0; i < in.size(); i += 512) {
            const int n = static_cast<int>(std::min<size_t>(512, in.size() - i));
            k.process(&in[i], &in[i], &l[i], &r[i], n);
        }
        const bool finite = sig::allFinite(l) && sig::allFinite(r);
        const float peak = std::max(sig::peak(l), sig::peak(r));
        const float tail = sig::rms(l, in.size() - sig::frames(2.f, kSR), in.size() - sig::frames(1.5f, kSR));
        if (!finite || peak >= 4.f || tail <= 1e-5f)
            std::printf("    %s: finite=%d peak=%g tail=%g\n", preset.name, finite, peak, tail);
        CHECK(finite);
        CHECK(peak < 4.f);
        CHECK(tail > 1e-5f);  // a reverb tail is still ringing after the last note
    }
}

TEST_CASE(presets_first_is_default_and_matches_parameter_defaults) {
    REQUIRE(std::strcmp(kPresets[0].name, "Default") == 0);
    for (const auto& spec : kParamSpecs) {
        if (kPresets[0].values[spec.address] != spec.def)
            std::printf("    Default.%s = %g, spec default %g\n", spec.identifier, kPresets[0].values[spec.address], spec.def);
        CHECK(kPresets[0].values[spec.address] == spec.def);
    }
}

TEST_CASE(presets_some_preset_uses_sub) {
    bool any = false;
    for (const auto& preset : kPresets) any = any || preset.values[SRParamSub] > 0.f;
    CHECK(any);
}

TEST_CASE(presets_night_sky_is_voiced_for_guitar) {
    // Against the same preset with Shimmer and Sub off, on strummed guitar: the pitch layers must
    // add warmth underneath, must not add chirpy brightness up top, and must keep the body.
    constexpr float kSR = 48000.f;
    const Preset* nightSky = nullptr;
    for (const auto& p : kPresets)
        if (std::strcmp(p.name, "Night Sky") == 0) nightSky = &p;
    REQUIRE(nightSky != nullptr);

    auto wet = [&](bool layers) {
        Kernel k;
        for (uint64_t p = 0; p < SRParamCount; ++p) k.setParameter(p, nightSky->values[p]);
        k.setParameter(SRParamMix, 100.f);
        if (!layers) {
            k.setParameter(SRParamShimmer, 0.f);
            k.setParameter(SRParamSub, 0.f);
        }
        k.prepare(kSR);
        sig::Buffer in;
        for (int i = 0; i < 4; ++i) sig::append(in, sig::strum(2.f, 0.5f, kSR));
        sig::append(in, sig::silence(8.f, kSR));
        sig::Buffer l(in.size()), r(in.size());
        for (size_t i = 0; i < in.size(); i += 512) {
            const int n = static_cast<int>(std::min<size_t>(512, in.size() - i));
            k.process(&in[i], &in[i], &l[i], &r[i], n);
        }
        return l;
    };
    const sig::Buffer with = wet(true), without = wet(false);
    const size_t from = sig::frames(4.f, kSR);
    auto change = [&](float lo, float hi) {
        return sig::bandEnergyDb(with, from, lo, hi, kSR) - sig::bandEnergyDb(without, from, lo, hi, kSR);
    };
    const float warmth = change(62.f, 250.f), body = change(250.f, 1000.f), top = change(2000.f, 8000.f);
    std::printf("    Night Sky pitch layers: warmth (62-250 Hz) %+.1f dB, body (250 Hz-1 kHz) %+.1f dB, top (2-8 kHz) %+.1f dB\n",
                warmth, body, top);
    CHECK(warmth > 2.f);
    CHECK(body > -3.f);
    CHECK(top < 0.f);
    CHECK(nightSky->values[SRParamResonance] == 0.f);  // no hidden resonant peak from the filter
}
