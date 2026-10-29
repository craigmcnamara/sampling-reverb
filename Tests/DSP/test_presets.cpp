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
    sig::Buffer in;
    for (int i = 0; i < 6; ++i) {
        sig::append(in, i % 3 == 2 ? sig::noiseBurst(0.3f, 0.5f, kSR, i + 1)
                                   : sig::toneBurst(i % 2 ? 330.f : 220.f, 0.3f, 0.5f, kSR));
        sig::append(in, sig::silence(0.5f, kSR));
    }
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
