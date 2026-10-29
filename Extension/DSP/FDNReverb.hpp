// Stereo 8-line feedback delay network with input diffusion, modulation, damping and shimmer.
#pragma once

#include "DSPUtil.hpp"

namespace sr {

// Two-grain crossfading delay-line pitch shifter.
struct PitchShifter {
    DelayLine d;
    float window = 1.f;
    float phase = 0.f;
    float ratio = 2.f;

    void prepare(float sampleRate) {
        window = 0.06f * sampleRate;
        d.allocate(static_cast<uint32_t>(window) + 8);
        phase = 0.f;
    }
    void setSemitones(float semis) { ratio = std::pow(2.f, semis / 12.f); }

    inline float process(float x) {
        d.write(x);
        phase += (1.f - ratio) / window;
        phase -= std::floor(phase);
        float p2 = phase + 0.5f;
        if (p2 >= 1.f) p2 -= 1.f;
        const float w1 = std::sin(kPi * phase);
        const float w2 = std::sin(kPi * p2);
        return d.read(phase * window) * (w1 * w1) + d.read(p2 * window) * (w2 * w2);
    }
};

class FDNReverb {
public:
    static constexpr int N = 8;

    struct Params {
        float decaySec = 4.f;
        float size = 0.6f;     // 0…1
        float tone = 0.6f;     // 0…1
        float texture = 0.5f;  // 0…1
        float mod = 0.3f;      // 0…1
        float shimmer = 0.f;   // 0…1
        float shimmerSemitones = 12.f;
        float sub = 0.f;       // 0…1, octave-down layer
        bool freeze = false;
    };

    void prepare(float sampleRate) {
        sr_ = sampleRate;
        static constexpr float kBaseMs[N] = {29.7f, 37.1f, 41.1f, 43.7f, 53.3f, 59.9f, 67.7f, 79.3f};
        const float maxMod = 0.0035f * sr_;
        for (int j = 0; j < N; ++j) {
            baseLen_[j] = kBaseMs[j] * 0.001f * sr_;
            lines_[j].allocate(static_cast<uint32_t>(baseLen_[j] * kMaxScale + maxMod) + 16);
            curLen_[j] = targetLen_[j] = baseLen_[j];
            lp_[j] = 0.f;
            lfoPhase_[j] = kTwoPi * j / N;
            lfoInc_[j] = kTwoPi * (0.1f + 0.8f * j / (N - 1)) / sr_;
        }
        static constexpr float kDiffMsL[4] = {4.77f, 3.59f, 12.73f, 9.31f};
        static constexpr float kDiffMsR[4] = {5.11f, 3.83f, 11.87f, 9.93f};
        for (int k = 0; k < 4; ++k) {
            diffL_[k].allocate(static_cast<uint32_t>(kDiffMsL[k] * 0.001f * sr_));
            diffR_[k].allocate(static_cast<uint32_t>(kDiffMsR[k] * 0.001f * sr_));
        }
        for (int k = 0; k < kShimmerLines; ++k) {
            shifters_[k].prepare(sr_);
            shifters_[k].phase = 0.25f * k;  // stagger grain boundaries across lines
            subShifters_[k].prepare(sr_);
            subShifters_[k].phase = 0.125f + 0.25f * k;
            subShifters_[k].setSemitones(kUndertoneSemitones[k]);
            subHp_[k].set(55.f, 0.707f, sr_);
            subHp_[k].reset();
            shimmerHp_[k] = 0.f;
        }
        shimmerHpCoef_ = 1.f - std::exp(-kTwoPi * 80.f / sr_);
        lenCoef_ = 1.f - std::exp(-1.f / (0.15f * sr_));
    }

    void setParams(const Params& p) {
        p_ = p;
        const float scale = kMinScale * std::pow(kMaxScale / kMinScale, clampf(p.size, 0.f, 1.f));
        const float rt60 = std::max(p.decaySec, 0.05f);
        for (int j = 0; j < N; ++j) {
            targetLen_[j] = baseLen_[j] * scale;
            gain_[j] = p.freeze ? 1.f : std::pow(10.f, -3.f * targetLen_[j] / (rt60 * sr_));
        }
        const float cutoff = 1500.f * std::pow(12.f, clampf(p.tone, 0.f, 1.f));
        lpCoef_ = 1.f - std::exp(-kTwoPi * std::min(cutoff, 0.45f * sr_) / sr_);
        const float g = 0.75f * clampf(p.texture, 0.f, 1.f);
        for (int k = 0; k < 4; ++k) diffL_[k].g = diffR_[k].g = g;
        modDepth_ = clampf(p.mod, 0.f, 1.f) * 0.003f * sr_;
        for (auto& sh : shifters_) sh.setSemitones(p.shimmerSemitones);
    }

    void process(const float* inL, const float* inR, float* outL, float* outR, int n) {
        // Shimmer and Sub live inside the feedback loop: on half the lines each, part of the
        // recirculating signal is swapped for its pitch-shifted copy. The swap is an equal-power
        // crossfade (cos/sin): the shifted copy is essentially uncorrelated with the original, so
        // this keeps the energy per pass, and the layers add richness instead of draining the
        // reverb's body. A linear crossfade lost up to half the energy every pass.
        const float shimmerAngle = clampf(p_.shimmer, 0.f, 1.f) * kMaxShimmerCrossfade;
        const float subAngle = clampf(p_.sub, 0.f, 1.f) * kMaxSubCrossfade;
        const float shimmerDry = std::cos(shimmerAngle), shimmerWet = std::sin(shimmerAngle);
        const float subDry = std::cos(subAngle), subWet = std::sin(subAngle);
        const bool freeze = p_.freeze;
        for (int i = 0; i < n; ++i) {
            float xl = freeze ? 0.f : inL[i];
            float xr = freeze ? 0.f : inR[i];

            for (int k = 0; k < 4; ++k) {
                xl = diffL_[k].process(xl);
                xr = diffR_[k].process(xr);
            }

            float o[N], s[N];
            for (int j = 0; j < N; ++j) {
                curLen_[j] += (targetLen_[j] - curLen_[j]) * lenCoef_;
                lfoPhase_[j] += lfoInc_[j];
                if (lfoPhase_[j] > kTwoPi) lfoPhase_[j] -= kTwoPi;
                o[j] = lines_[j].read(curLen_[j] + modDepth_ * std::sin(lfoPhase_[j]));
                lp_[j] += lpCoef_ * (o[j] - lp_[j]);
                s[j] = freeze ? o[j] : lp_[j];
            }
            hadamard8(s);
            // Both shifted paths are high-passed: a grain shifter leaves very low frequencies (below
            // its grain rate) almost unshifted, so they'd stay correlated with the original and grow
            // on every pass under an equal-power crossfade.
            if (shimmerAngle > 0.f) {
                for (int k = 0; k < kShimmerLines; ++k) {
                    const float up = shifters_[k].process(s[k]);
                    shimmerHp_[k] += shimmerHpCoef_ * (up - shimmerHp_[k]);
                    s[k] = shimmerDry * s[k] + shimmerWet * (up - shimmerHp_[k]);
                }
            }
            // Sub follows the undertone series (f/2, f/2, f/3, f/4) on the other half of the lines, and
            // cascades on every pass. A 2-pole 55 Hz high-pass keeps the undertones of low notes (which
            // land at 20–40 Hz) from turning into rumble; the warmth stays in the 60–250 Hz range.
            if (subAngle > 0.f) {
                for (int k = 0; k < kShimmerLines; ++k) {
                    float lp, hp;
                    subHp_[k].process(subShifters_[k].process(s[kShimmerLines + k]), lp, hp);
                    s[kShimmerLines + k] = subDry * s[kShimmerLines + k] + subWet * hp;
                }
            }
            for (int j = 0; j < N; ++j) {
                const float in = (j & 1) ? xr : xl;
                const float sign = (j & 2) ? -1.f : 1.f;
                lines_[j].write(safeLimit(sign * in * 0.5f + gain_[j] * s[j]));
            }
            const float l = (o[0] - o[2] + o[4] - o[6]) * 0.5f;
            const float r = (o[1] - o[3] + o[5] - o[7]) * 0.5f;
            outL[i] = l;
            outR[i] = r;
        }
    }

    void clear() {
        for (auto& l : lines_) l.clear();
        for (auto& a : diffL_) a.d.clear();
        for (auto& a : diffR_) a.d.clear();
        for (auto& sh : shifters_) sh.d.clear();
        for (auto& sh : subShifters_) sh.d.clear();
        for (auto& f : subHp_) f.reset();
        for (float& v : shimmerHp_) v = 0.f;
        for (float& v : lp_) v = 0.f;
    }

private:
    static constexpr int kShimmerLines = N / 2;  // shimmer uses lines 0–3, sub uses lines 4–7
    // Crossfade angle at 100%. Shimmer moves energy up into the range Tone damps fastest, so it
    // gets a gentler maximum (sin = 0.71) than Sub (sin = 0.92) to keep the reverb's body.
    static constexpr float kMaxShimmerCrossfade = 0.5f * kPi / 2;
    static constexpr float kMaxSubCrossfade = 0.75f * kPi / 2;
    // Undertone series: f/2, f/2, f/3 (an octave and a fifth down) and f/4 (two octaves down).
    static constexpr float kUndertoneSemitones[kShimmerLines] = {-12.f, -12.f, -19.0196f, -24.f};
    static constexpr float kMinScale = 0.25f;
    static constexpr float kMaxScale = 2.f;

    static inline void hadamard8(float* x) {
        for (int h = 1; h < N; h <<= 1)
            for (int i = 0; i < N; i += h * 2)
                for (int j = i; j < i + h; ++j) {
                    const float a = x[j], b = x[j + h];
                    x[j] = a + b;
                    x[j + h] = a - b;
                }
        for (int j = 0; j < N; ++j) x[j] *= 0.35355339f;  // 1/sqrt(8)
    }

    float sr_ = 48000.f;
    Params p_;
    DelayLine lines_[N];
    float baseLen_[N] {}, curLen_[N] {}, targetLen_[N] {}, gain_[N] {}, lp_[N] {};
    float lfoPhase_[N] {}, lfoInc_[N] {};
    float lpCoef_ = 0.5f, lenCoef_ = 0.001f, modDepth_ = 0.f;
    Allpass diffL_[4], diffR_[4];
    PitchShifter shifters_[kShimmerLines];
    PitchShifter subShifters_[kShimmerLines];
    SVF subHp_[kShimmerLines];
    float shimmerHp_[kShimmerLines] {};
    float shimmerHpCoef_ = 0.f;
};

} // namespace sr
