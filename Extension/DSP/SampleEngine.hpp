// Onset detection, feature analysis, the sample pool (with retention) and the voice bank.
#pragma once

#include "DSPUtil.hpp"
#include "VisualTypes.h"

namespace sr {

struct Features {
    float bright = 0.f;    // 0…1
    float clarity = 0.f;   // 0…1
    float midi = -1.f;     // < 0 when unpitched
    float loudDb = -120.f;
    float x = 0.5f, y = 0.5f;
};

struct EngineParams {
    float retention = 0.5f;      // 0…1, ≥ 0.995 = keep forever
    float sensitivityDb = -40.f;
    float match = 0.4f;          // 0…1 tolerance
    float harmony = 0.3f;        // 0 = similar, 1 = complementary
    float lengthSec = 0.8f;
    int density = 4;
    float focusX = 0.5f, focusY = 0.5f, focusAmount = 0.f;
    bool freeze = false;
};

class SampleEngine {
public:
    static constexpr int kSlots = SR_MAX_SLOTS;
    static constexpr int kVoices = 12;
    static constexpr float kMaxLengthSec = 2.f;

    void prepare(float sampleRate) {
        sr_ = sampleRate;
        preroll_ = static_cast<uint32_t>(0.01f * sr_);
        window_ = sr_ > 64000.f ? 4096u : 2048u;
        maxLen_ = static_cast<uint32_t>(kMaxLengthSec * sr_) + preroll_;
        const uint32_t histSize = nextPow2(maxLen_ + window_ + 16384);
        histL_.assign(histSize, 0.f);
        histR_.assign(histSize, 0.f);
        histMask_ = histSize - 1;
        poolL_.assign(static_cast<size_t>(maxLen_) * kSlots, 0.f);
        poolR_.assign(static_cast<size_t>(maxLen_) * kSlots, 0.f);
        for (int s = 0; s < kSlots; ++s) {
            slots_[s] = Slot {};
            slots_[s].L = &poolL_[static_cast<size_t>(s) * maxLen_];
            slots_[s].R = &poolR_[static_cast<size_t>(s) * maxLen_];
        }
        for (auto& v : voices_) v = Voice {};
        mono_.assign(window_, 0.f);
        dec_.assign(window_, 0.f);
        acf_.assign(window_, 0.f);

        frame_ = 0;
        fast_ = slow_ = 0.f;
        holdoff_ = 0;
        analyzing_ = capturing_ = false;
        fastAtt_ = std::exp(-1.f / (0.001f * sr_));
        fastRel_ = std::exp(-1.f / (0.010f * sr_));
        slowCoef_ = std::exp(-1.f / (0.150f * sr_));
        meterDecay_ = std::exp(-1.f / (0.08f * sr_));
        attackInc_ = 1.f / (0.005f * sr_);
        releaseInc_ = 1.f / (0.010f * sr_);
    }

    void setParams(const EngineParams& p) { p_ = p; }

    // Analyzes the input and renders triggered voices into out (which does not include the input).
    void process(const float* inL, const float* inR, float* outL, float* outR, int n) {
        const float thresh = dbToGain(p_.sensitivityDb);
        for (int i = 0; i < n; ++i) {
            const float l = inL[i], r = inR[i];
            const uint32_t idx = static_cast<uint32_t>(frame_) & histMask_;
            histL_[idx] = l;
            histR_[idx] = r;

            const float m = 0.5f * (std::fabs(l) + std::fabs(r));
            fast_ = m + (fast_ - m) * (m > fast_ ? fastAtt_ : fastRel_);
            slow_ = fast_ + (slow_ - fast_) * slowCoef_;
            ++frame_;

            if (holdoff_ > 0) {
                --holdoff_;
            } else if (!analyzing_ && fast_ > thresh && fast_ > slow_ * 2.f) {
                analyzing_ = true;
                onsetFrame_ = frame_ - 1;
                holdoff_ = static_cast<uint32_t>(0.08f * sr_);
            }
            if (analyzing_ && frame_ >= onsetFrame_ + window_) {
                analyzing_ = false;
                handleOnset();
            }
            if (capturing_ && frame_ >= capEnd_) {
                capturing_ = false;
                finishCapture();
            }

            float ol = 0.f, orr = 0.f;
            for (auto& v : voices_) {
                if (!v.active) continue;
                const Slot& s = slots_[v.slot];
                if (v.pos >= s.length) {
                    v.active = false;
                    continue;
                }
                v.env += v.envInc;
                if (v.env >= 1.f) {
                    v.env = 1.f;
                    v.envInc = 0.f;
                } else if (v.env <= 0.f) {
                    v.active = false;
                    continue;
                }
                const float sl = s.L[v.pos] * v.env;
                const float sr = s.R[v.pos] * v.env;
                ++v.pos;
                ol += sl * v.gl;
                orr += sr * v.gr;
                v.meter = std::max(0.5f * (std::fabs(sl) + std::fabs(sr)), v.meter * meterDecay_);
            }
            outL[i] = ol;
            outR[i] = orr;
        }
    }

    // Ages the pool. Call once per processed block.
    void advanceLife(float seconds) {
        const float ml = maxLife();
        if (p_.freeze || std::isinf(ml)) return;
        for (int s = 0; s < kSlots; ++s) {
            Slot& slot = slots_[s];
            if (!slot.active) continue;
            slot.life -= seconds;
            if (slot.life <= 0.f) {
                slot.life = 0.f;
                if (!isPlaying(s)) slot.active = false;
            }
        }
    }

    int poolCount() const {
        int c = 0;
        for (const auto& s : slots_) c += s.active ? 1 : 0;
        return c;
    }

    void fillVisual(SRSlotVisual* out) const {
        const float ml = maxLife();
        for (int s = 0; s < kSlots; ++s) {
            const Slot& slot = slots_[s];
            SRSlotVisual& v = out[s];
            v.active = slot.active ? 1 : 0;
            v.generation = slot.generation;
            v.triggerCount = slot.triggerCount;
            v.x = slot.f.x;
            v.y = slot.f.y;
            v.clarity = slot.f.clarity;
            v.life = std::isinf(ml) ? 1.f : clampf(slot.life / ml, 0.f, 2.f);
            float level = 0.f;
            for (const auto& voice : voices_)
                if (voice.active && voice.slot == s) level = std::max(level, voice.meter / std::max(slot.peak, 1e-4f));
            v.level = clampf(level, 0.f, 1.f);
        }
    }

    // Introspection (used by tests).
    uint64_t totalTriggers() const { return totalTriggers_; }
    uint64_t totalCaptures() const { return totalCaptures_; }
    const Features& lastFeatures() const { return lastFeatures_; }
    int activeVoiceCount() const {
        int c = 0;
        for (const auto& v : voices_) c += (v.active && !v.releasing) ? 1 : 0;
        return c;
    }

private:
    struct Slot {
        bool active = false;
        uint32_t generation = 0;
        uint32_t triggerCount = 0;
        float* L = nullptr;
        float* R = nullptr;
        uint32_t length = 0;
        float peak = 0.f;
        Features f;
        float life = 0.f;
        uint64_t lastTrigger = 0;
    };

    struct Voice {
        bool active = false;
        bool releasing = false;
        int slot = 0;
        uint32_t pos = 0;
        float env = 0.f, envInc = 0.f;
        float gl = 1.f, gr = 1.f;
        float meter = 0.f;
        uint64_t start = 0;
    };

    float maxLife() const {
        if (p_.retention >= 0.995f) return INFINITY;
        return 3.f * std::pow(100.f, clampf(p_.retention, 0.f, 1.f));
    }

    bool isPlaying(int s) const {
        for (const auto& v : voices_)
            if (v.active && v.slot == s) return true;
        return false;
    }

    // ---- Analysis --------------------------------------------------------------------------

    Features analyze(uint64_t start) {
        Features f;
        const uint32_t W = window_;
        const float hpA = 1.f - std::exp(-kTwoPi * 1500.f / sr_);
        double total = 0.0, high = 0.0;
        float lp = 0.f;
        for (uint32_t k = 0; k < W; ++k) {
            const uint32_t idx = static_cast<uint32_t>(start + k) & histMask_;
            const float x = 0.5f * (histL_[idx] + histR_[idx]);
            mono_[k] = x;
            lp += hpA * (x - lp);
            const float h = x - lp;
            total += x * x;
            high += h * h;
        }
        f.loudDb = gainToDb(static_cast<float>(std::sqrt(total / W)));
        const float ratio = total > 1e-12 ? static_cast<float>(high / total) : 0.f;
        f.bright = clampf(std::sqrt(ratio) * 1.2f, 0.f, 1.f);

        // Pitch: normalized autocorrelation on a ~12 kHz decimated copy.
        const int D = std::max(1, static_cast<int>(std::lround(sr_ / 12000.f)));
        const int N = static_cast<int>(W) / D;
        const float dsr = sr_ / D;
        float mean = 0.f;
        for (int j = 0; j < N; ++j) {
            float s = 0.f;
            for (int q = 0; q < D; ++q) s += mono_[j * D + q];
            dec_[j] = s / D;
            mean += dec_[j];
        }
        mean /= N;
        for (int j = 0; j < N; ++j) dec_[j] -= mean;

        const int minLag = std::max(2, static_cast<int>(dsr / 1200.f));
        const int maxLag = std::min(static_cast<int>(dsr / 60.f) + 1, N - 64);
        float rmax = 0.f;
        for (int tau = minLag; tau <= maxLag; ++tau) {
            float num = 0.f, e1 = 0.f, e2 = 0.f;
            for (int n = 0; n < N - tau; ++n) {
                const float a = dec_[n], b = dec_[n + tau];
                num += a * b;
                e1 += a * a;
                e2 += b * b;
            }
            const float r = num / std::sqrt(e1 * e2 + 1e-12f);
            acf_[tau] = r;
            rmax = std::max(rmax, r);
        }
        int best = -1;
        for (int tau = minLag + 1; tau < maxLag; ++tau) {
            if (acf_[tau] >= 0.9f * rmax && acf_[tau] >= acf_[tau - 1] && acf_[tau] >= acf_[tau + 1]) {
                best = tau;
                break;
            }
        }
        f.clarity = clampf(rmax, 0.f, 1.f);
        if (best > 0 && f.clarity >= 0.5f) {
            const float a = acf_[best - 1], b = acf_[best], c = acf_[best + 1];
            const float denom = a - 2.f * b + c;
            const float shift = std::fabs(denom) > 1e-9f ? 0.5f * (a - c) / denom : 0.f;
            const float hz = dsr / (best + shift);
            f.midi = 69.f + 12.f * std::log2(hz / 440.f);
        }

        f.x = f.bright;
        f.y = f.midi >= 0.f ? std::fmod(f.midi + 0.5f, 12.f) / 12.f : clampf((f.loudDb + 60.f) / 60.f, 0.f, 1.f);
        return f;
    }

    float distance(const Features& a, const Features& b) const {
        static constexpr float kSimilar[12] = {0.f, 1.f, 1.f, 1.f, 1.f, 1.f, 1.f, 1.f, 1.f, 1.f, 1.f, 1.f};
        static constexpr float kComplement[12] = {0.3f, 1.f, 0.6f, 0.15f, 0.15f, 0.05f, 1.f, 0.05f, 0.15f, 0.15f, 0.6f, 1.f};
        float d = std::fabs(a.bright - b.bright) + 0.5f * std::fabs(a.clarity - b.clarity);
        const bool pa = a.midi >= 0.f, pb = b.midi >= 0.f;
        if (pa && pb) {
            const int iv = ((static_cast<int>(std::lround(a.midi - b.midi)) % 12) + 12) % 12;
            d += 0.6f * lerpf(kSimilar[iv], kComplement[iv], p_.harmony);
        } else if (pa != pb) {
            d += 0.3f;
        }
        const float dx = b.x - p_.focusX, dy = b.y - p_.focusY;
        d += p_.focusAmount * (std::sqrt(dx * dx + dy * dy) - 0.3f);
        return d;
    }

    // ---- Pool -------------------------------------------------------------------------------

    void handleOnset() {
        const Features f = analyze(onsetFrame_);
        lastFeatures_ = f;
        const uint64_t retrigGuard = static_cast<uint64_t>(0.1f * sr_);
        int best = -1;
        float bestD = 0.03f + 0.57f * p_.match;
        for (int s = 0; s < kSlots; ++s) {
            const Slot& slot = slots_[s];
            if (!slot.active || frame_ - slot.lastTrigger < retrigGuard) continue;
            const float d = distance(f, slot.f);
            if (d < bestD) {
                bestD = d;
                best = s;
            }
        }
        if (best >= 0) {
            trigger(best);
        } else if (!p_.freeze && !capturing_) {
            capturing_ = true;
            capStart_ = onsetFrame_ >= preroll_ ? onsetFrame_ - preroll_ : 0;
            const uint32_t len = static_cast<uint32_t>(clampf(p_.lengthSec, 0.05f, kMaxLengthSec) * sr_);
            capEnd_ = std::min<uint64_t>(onsetFrame_ + len, capStart_ + maxLen_);
            capFeatures_ = f;
        }
    }

    void finishCapture() {
        int target = -1;
        for (int s = 0; s < kSlots && target < 0; ++s)
            if (!slots_[s].active) target = s;
        if (target < 0) {
            float lowest = INFINITY;
            for (int s = 0; s < kSlots; ++s) {
                if (isPlaying(s)) continue;
                if (slots_[s].life < lowest) {
                    lowest = slots_[s].life;
                    target = s;
                }
            }
        }
        if (target < 0) return;

        Slot& slot = slots_[target];
        const uint32_t len = static_cast<uint32_t>(capEnd_ - capStart_);
        const uint32_t fadeIn = std::max<uint32_t>(1, std::min(preroll_, len / 2));
        const uint32_t fadeOut = std::max<uint32_t>(1, std::min(static_cast<uint32_t>(0.03f * sr_), len / 2));
        float peak = 0.f;
        for (uint32_t k = 0; k < len; ++k) {
            const uint32_t idx = static_cast<uint32_t>(capStart_ + k) & histMask_;
            float g = 1.f;
            if (k < fadeIn) g = static_cast<float>(k) / fadeIn;
            if (k >= len - fadeOut) g *= static_cast<float>(len - k) / fadeOut;
            slot.L[k] = histL_[idx] * g;
            slot.R[k] = histR_[idx] * g;
            peak = std::max(peak, 0.5f * (std::fabs(slot.L[k]) + std::fabs(slot.R[k])));
        }
        const float ml = maxLife();
        slot.active = true;
        slot.generation++;
        slot.length = len;
        slot.peak = peak;
        slot.f = capFeatures_;
        slot.life = std::isinf(ml) ? 1.f : ml;
        slot.lastTrigger = frame_;
        totalCaptures_++;
    }

    void trigger(int s) {
        Slot& slot = slots_[s];
        slot.triggerCount++;
        slot.lastTrigger = frame_;
        const float ml = maxLife();
        if (!std::isinf(ml)) slot.life = std::min(slot.life + 0.5f * ml, 2.f * ml);
        totalTriggers_++;

        // Respect Density: release the oldest sounding voices beyond the cap.
        const int cap = std::max(1, std::min(p_.density, kVoices));
        for (;;) {
            int sounding = 0, oldest = -1;
            for (int v = 0; v < kVoices; ++v) {
                if (!voices_[v].active || voices_[v].releasing) continue;
                ++sounding;
                if (oldest < 0 || voices_[v].start < voices_[oldest].start) oldest = v;
            }
            if (sounding < cap || oldest < 0) break;
            voices_[oldest].releasing = true;
            voices_[oldest].envInc = -releaseInc_;
        }
        int free = -1;
        for (int v = 0; v < kVoices && free < 0; ++v)
            if (!voices_[v].active) free = v;
        if (free < 0) {
            free = 0;
            for (int v = 1; v < kVoices; ++v)
                if (voices_[v].env < voices_[free].env) free = v;
        }
        Voice& v = voices_[free];
        v.active = true;
        v.releasing = false;
        v.slot = s;
        v.pos = 0;
        v.env = 0.f;
        v.envInc = attackInc_;
        v.meter = 0.f;
        v.start = frame_;
        const float angle = (rng_.bipolar() * 0.4f + 1.f) * kPi * 0.25f;
        v.gl = std::cos(angle) * 1.41421356f;
        v.gr = std::sin(angle) * 1.41421356f;
    }

    float sr_ = 48000.f;
    EngineParams p_;

    std::vector<float> histL_, histR_;
    uint32_t histMask_ = 0;
    uint64_t frame_ = 0;

    float fast_ = 0.f, slow_ = 0.f, fastAtt_ = 0.f, fastRel_ = 0.f, slowCoef_ = 0.f;
    uint32_t holdoff_ = 0;
    bool analyzing_ = false;
    uint64_t onsetFrame_ = 0;
    uint32_t window_ = 2048, preroll_ = 480, maxLen_ = 0;

    bool capturing_ = false;
    uint64_t capStart_ = 0, capEnd_ = 0;
    Features capFeatures_;

    std::vector<float> poolL_, poolR_;
    Slot slots_[kSlots];
    Voice voices_[kVoices];
    float meterDecay_ = 0.f, attackInc_ = 0.f, releaseInc_ = 0.f;
    Rng rng_;

    std::vector<float> mono_, dec_, acf_;
    Features lastFeatures_;
    uint64_t totalTriggers_ = 0, totalCaptures_ = 0;
};

} // namespace sr
