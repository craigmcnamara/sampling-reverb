// Runs the original C++ kernel and the Rust kernel (through its C ABI) side by side on the same
// input, parameter changes and host clock, and requires bit-identical output.
//
// Both use the same libm calls in the same order; the C++ side is built with -ffp-contract=off
// because Rust never fuses multiply-adds.
#include <cmath>
#include <cstdio>
#include <cstring>
#include <random>
#include <vector>

#include "Presets.hpp"
#include "SamplingReverbKernel.hpp"
#include "Signals.hpp"
#include "sr_dsp.h"

namespace {

int failures = 0;

struct Scenario {
    const char* name;
    float sampleRate;
    uint32_t seed;
    bool host;            // host tempo and position, else the free-running Tempo parameter
    bool randomBlocks;    // block sizes 1..1024, else 512
    bool inPlace;         // output buffers are the input buffers, as in the AU
    bool mono;            // inL == inR
    float seconds;
};

sig::Buffer makeInput(float sr, uint32_t seed, float seconds) {
    std::mt19937 rng(seed);
    sig::Buffer in;
    while (in.size() < sig::frames(seconds, sr)) {
        switch (rng() % 4) {
            case 0: sig::append(in, sig::strum(0.8f, 0.4f, sr)); break;
            case 1: sig::append(in, sig::toneBurst(82.f * (1 + rng() % 9), 0.4f, 0.5f, sr)); break;
            case 2: sig::append(in, sig::noiseBurst(0.2f, 0.4f, sr, rng())); break;
            default: sig::append(in, sig::silence(0.3f, sr)); break;
        }
    }
    in.resize(sig::frames(seconds, sr));
    return in;
}

void run(const Scenario& sc) {
    const float SR = sc.sampleRate;
    const sig::Buffer inL = makeInput(SR, sc.seed, sc.seconds);
    sig::Buffer inR = inL;
    for (size_t i = 0; i < inR.size(); ++i) inR[i] *= 0.7f + 0.3f * std::sin(i * 0.0001f);  // a little stereo

    sr::Kernel cpp;
    cpp.prepare(SR);
    SRKernel* rs = sr_kernel_new();
    sr_kernel_prepare(rs, SR);

    std::mt19937 rng(sc.seed * 31 + 7);
    double tempo = 97.3, beat = 1.37;
    size_t nextChange = 0;
    std::vector<float> aL(4096), aR(4096), bL(4096), bR(4096);
    size_t i = 0;
    while (i < inL.size()) {
        const int n = static_cast<int>(std::min<size_t>(sc.randomBlocks ? 1 + rng() % 1024 : 512, inL.size() - i));
        if (i >= nextChange) {
            nextChange = i + static_cast<size_t>(SR * (0.2f + (rng() % 1000) / 800.f));
            const int preset = rng() % sr::kPresetCount;
            auto set = [&](uint64_t p, float v) {
                cpp.setParameter(p, v);
                sr_kernel_set_parameter(rs, p, v);
            };
            for (uint64_t p = 0; p < SRParamCount; ++p)
                if (!sr::presetLeavesAlone(p)) set(p, sr::kPresets[preset].values[p]);
            set(SRParamSync, rng() % 2);
            set(SRParamQuantize, rng() % 2);
            set(SRParamGrid, rng() % 8);
            set(SRParamFreeze, rng() % 10 == 0);
            set(SRParamFilter, static_cast<float>(static_cast<int>(rng() % 201) - 100));
            set(SRParamMatch, static_cast<float>(rng() % 101));
            set(SRParamFocusAmount, static_cast<float>(rng() % 101));
            tempo = 40.0 + (rng() % 20000) / 100.0;  // fractional tempos
            set(SRParamTempo, static_cast<float>(tempo));
        }
        sr::HostTime ht;
        SRHostTime rt {};
        if (sc.host) {
            ht.hasTempo = rt.hasTempo = true;
            ht.tempo = rt.tempo = tempo;
            ht.hasPosition = rt.hasPosition = true;
            ht.beat = rt.beat = beat;
            ht.downbeat = rt.downbeat = std::floor(beat / 4) * 4;
            ht.barBeats = rt.barBeats = 4.0;
            ht.hasTransport = rt.hasTransport = true;
            ht.playing = rt.playing = (i / 48000) % 7 != 3;  // stop now and then
        }
        cpp.setHostTime(ht);

        // C++: separate output buffers. Rust: in place (or not), as the scenario says.
        cpp.process(&inL[i], sc.mono ? &inL[i] : &inR[i], aL.data(), aR.data(), n);
        std::memcpy(bL.data(), &inL[i], n * sizeof(float));
        std::memcpy(bR.data(), sc.mono ? &inL[i] : &inR[i], n * sizeof(float));
        if (sc.inPlace) {
            sr_kernel_process(rs, &rt, bL.data(), sc.mono ? bL.data() : bR.data(), bL.data(), bR.data(), n);
            // With a mono in-place input, the right output must not be written before it's read.
        } else {
            std::vector<float> cL(bL.begin(), bL.begin() + n), cR(bR.begin(), bR.begin() + n);
            sr_kernel_process(rs, &rt, cL.data(), sc.mono ? cL.data() : cR.data(), bL.data(), bR.data(), n);
        }
        for (int j = 0; j < n; ++j) {
            if (std::memcmp(&aL[j], &bL[j], sizeof(float)) != 0 || std::memcmp(&aR[j], &bR[j], sizeof(float)) != 0) {
                std::printf("  FAIL %s: first difference at %.4f s (frame %zu): C++ %.9g %.9g, Rust %.9g %.9g\n", sc.name,
                            (i + j) / SR, i + j, aL[j], aR[j], bL[j], bR[j]);
                ++failures;
                sr_kernel_free(rs);
                return;
            }
        }
        beat += n * tempo / 60.0 / SR;
        i += n;
    }

    uint64_t trig = 0, caps = 0;
    sr_kernel_debug_counts(rs, &trig, &caps);
    SRSlotVisual vc[SR_MAX_SLOTS], vr[SR_MAX_SLOTS];
    float scL[SR_SCOPE_POINTS], scR[SR_SCOPE_POINTS], srL[SR_SCOPE_POINTS], srR[SR_SCOPE_POINTS];
    const auto summary = cpp.readVisual(vc, SR_MAX_SLOTS, scL, scR, SR_SCOPE_POINTS);
    bool frozen = false;
    const int pool = sr_kernel_read_visual(rs, vr, SR_MAX_SLOTS, srL, srR, SR_SCOPE_POINTS, &frozen);
    const bool same = trig == cpp.engine().totalTriggers() && caps == cpp.engine().totalCaptures() &&
                      pool == summary.poolCount && frozen == summary.frozen &&
                      std::memcmp(vc, vr, sizeof vc) == 0 && std::memcmp(scL, srL, sizeof scL) == 0 &&
                      std::memcmp(scR, srR, sizeof scR) == 0;
    const auto clockC = cpp.clock();
    const auto clockR = sr_kernel_clock(rs);
    if (!same || clockC.tempo != clockR.tempo || clockC.beat != clockR.beat || clockC.fromHost != clockR.fromHost) {
        std::printf("  FAIL %s: state differs (triggers %llu/%llu, captures %llu/%llu, pool %d/%d)\n", sc.name,
                    (unsigned long long)cpp.engine().totalTriggers(), (unsigned long long)trig,
                    (unsigned long long)cpp.engine().totalCaptures(), (unsigned long long)caps, summary.poolCount, pool);
        ++failures;
    } else {
        std::printf("  ok   %s: %.0f s identical, %llu triggers, %llu captures\n", sc.name, sc.seconds,
                    (unsigned long long)trig, (unsigned long long)caps);
    }
    sr_kernel_free(rs);
}

void checkMetadata() {
    int bad = 0;
    bad += sr_param_count() != SRParamCount;
    for (const auto& spec : sr::kParamSpecs) {
        const SRParamSpec r = sr_param_spec(spec.address);
        bad += std::strcmp(r.identifier, spec.identifier) != 0 || std::strcmp(r.name, spec.name) != 0 ||
               r.min != spec.min || r.max != spec.max || r.def != spec.def ||
               r.unit != static_cast<uint32_t>(spec.unit) || r.exponential != spec.exponential;
        bad += sr_preset_leaves_alone(spec.address) != sr::presetLeavesAlone(spec.address);
    }
    bad += sr_preset_count() != sr::kPresetCount;
    for (int i = 0; i < sr::kPresetCount; ++i) {
        bad += std::strcmp(sr_preset_name(i), sr::kPresets[i].name) != 0;
        for (uint64_t p = 0; p < SRParamCount; ++p) bad += sr_preset_value(i, p) != sr::kPresets[i].values[p];
    }
    auto names = [&](uint64_t address, const char* const* list, int count) {
        bad += sr_param_value_name_count(address) != count;
        for (int i = 0; i < count; ++i) bad += std::strcmp(sr_param_value_name(address, i), list[i]) != 0;
    };
    names(SRParamInterval, sr::kIntervalNames, sr::kIntervalCount);
    names(SRParamGrid, sr::kGridNames, sr::kGridCount);
    names(SRParamPreDelayDiv, sr::kPreDelayDivNames, sr::kPreDelayDivCount);
    names(SRParamLengthDiv, sr::kLengthDivNames, sr::kLengthDivCount);
    bad += sr_param_value_name(SRParamGrid, 99) != nullptr || sr_param_spec(999).identifier != nullptr;
    std::printf("  %s metadata: parameters, presets and value names\n", bad ? "FAIL" : "ok  ");
    failures += bad != 0;
}

void checkHalfFloats() {
    // Every float bit pattern in a sweep of the range, plus the edges, must round like _Float16.
    int bad = 0;
    auto check = [&](float x) {
        const float c = static_cast<float>(static_cast<_Float16>(x));
        const float r = sr_debug_f16_round_trip(x);
        if (std::memcmp(&c, &r, sizeof c) != 0 && !(std::isnan(c) && std::isnan(r))) {
            if (bad++ < 5) std::printf("    f16 %.9g: C++ %.9g, Rust %.9g\n", x, c, r);
        }
    };
    for (uint64_t bits = 0; bits <= 0xffffffffull; bits += 97) {
        float x;
        const uint32_t b = static_cast<uint32_t>(bits);
        std::memcpy(&x, &b, sizeof x);
        check(x);
    }
    for (float x : {0.f, -0.f, 65504.f, 65519.f, 65520.f, 1e-8f, 5.96e-8f, 2.98e-8f, 2.99e-8f, 6.1e-5f, INFINITY, -INFINITY})
        check(x);
    std::printf("  %s half floats round like _Float16 (%d mismatches)\n", bad ? "FAIL" : "ok  ", bad);
    failures += bad != 0;
}

} // namespace

int main() {
    std::printf("Parity: C++ reference vs Rust\n");
    checkMetadata();
    checkHalfFloats();
    const Scenario scenarios[] = {
        {"48k host, 512 blocks", 48000.f, 1, true, false, false, false, 60.f},
        {"48k free-running, random blocks, in place", 48000.f, 2, false, true, true, false, 60.f},
        {"44.1k host, random blocks, in place, mono", 44100.f, 3, true, true, true, true, 45.f},
        {"96k host, random blocks", 96000.f, 4, true, true, false, false, 30.f},
        {"48k free-running, 512 blocks, mono", 48000.f, 5, false, false, false, true, 45.f},
        {"88.2k host, random blocks, in place", 88200.f, 6, true, true, true, false, 30.f},
    };
    for (const auto& sc : scenarios) run(sc);
    std::printf("%s\n", failures ? "PARITY FAILED" : "parity: identical");
    return failures ? 1 : 0;
}
