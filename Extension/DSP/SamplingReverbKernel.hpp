// Top-level DSP kernel: parameters, sample engine → pre-delay → FDN → filter → mix, visual publishing.
#pragma once

#include <atomic>
#include <cstring>

#include "DSPUtil.hpp"
#include "FDNReverb.hpp"
#include "ParamIDs.h"
#include "ParamInfo.hpp"
#include "SampleEngine.hpp"
#include "TripleBuffer.hpp"
#include "VisualTypes.h"

namespace sr {

// What the host reports at the start of a render block. Any part may be missing.
struct HostTime {
    bool hasTempo = false;
    double tempo = 120.0;
    bool hasPosition = false;
    double beat = 0.0;            // quarter notes
    double downbeat = 0.0;        // beat of the current bar's first beat
    double barBeats = 4.0;        // quarter notes per bar
    bool hasTransport = false;
    bool playing = false;
};

struct ClockSummary {
    double tempo = 120.0;
    double beat = 0.0;
    bool fromHost = false;
};

struct VisualSummary {
    int poolCount = 0;
    bool frozen = false;
};

class Kernel {
public:
    static constexpr int kChunk = 256;

    Kernel() {
        for (const auto& spec : kParamSpecs) params_[spec.address].store(spec.def, std::memory_order_relaxed);
    }

    // ---- Any thread ------------------------------------------------------------------------

    void setParameter(uint64_t address, float value) {
        if (address < SRParamCount) params_[address].store(value, std::memory_order_relaxed);
    }
    float getParameter(uint64_t address) const {
        return address < SRParamCount ? params_[address].load(std::memory_order_relaxed) : 0.f;
    }
    void setBypass(bool bypass) { bypass_.store(bypass, std::memory_order_relaxed); }

    ClockSummary clock() const {
        return {shownTempo_.load(std::memory_order_relaxed), shownBeat_.load(std::memory_order_relaxed),
                shownFromHost_.load(std::memory_order_relaxed)};
    }

    // ---- Non-real-time ---------------------------------------------------------------------

    void prepare(double sampleRate) {
        sr_ = static_cast<float>(sampleRate);
        reverb_.prepare(sr_);
        engine_.prepare(sr_);
        preL_.allocate(static_cast<uint32_t>(0.51f * sr_));
        preR_.allocate(static_cast<uint32_t>(0.51f * sr_));
        filterL_.reset();
        filterR_.reset();
        for (auto& v : scopeL_) v = 0.f;
        for (auto& v : scopeR_) v = 0.f;
        scopeW_ = 0;
        first_ = true;
        beat_ = 0.0;
        barOrigin_ = 0.0;
        lastHostBeat_ = -1.0;
    }

    // ---- UI thread (single consumer) -------------------------------------------------------

    VisualSummary readVisual(SRSlotVisual* slots, int slotCount, float* scopeL, float* scopeR, int scopeCount) {
        const VisualState& v = visual_.read();
        if (slots) std::memcpy(slots, v.slots, sizeof(SRSlotVisual) * std::min(slotCount, SR_MAX_SLOTS));
        const int n = std::min(scopeCount, SR_SCOPE_POINTS);
        if (scopeL) std::memcpy(scopeL, v.scopeL + (SR_SCOPE_POINTS - n), sizeof(float) * n);
        if (scopeR) std::memcpy(scopeR, v.scopeR + (SR_SCOPE_POINTS - n), sizeof(float) * n);
        return {v.poolCount, v.frozen != 0};
    }

    // ---- Real-time -------------------------------------------------------------------------

    // Call before process() with what the host reported for this block.
    void setHostTime(const HostTime& t) { host_ = t; }

    // In-place safe: outL/outR may alias inL/inR, and inL may equal inR (mono).
    void process(const float* inL, const float* inR, float* outL, float* outR, int frames) {
        beginClock();
        if (bypass_.load(std::memory_order_relaxed)) {
            if (outL != inL) std::memmove(outL, inL, sizeof(float) * frames);
            if (outR != inR) std::memmove(outR, inR, sizeof(float) * frames);
            return;
        }
        for (int done = 0; done < frames;) {
            const int n = std::min(kChunk, frames - done);
            engine_.setClock({beat_, beatsPerFrame_, barOrigin_});
            processChunk(inL + done, inR + done, outL + done, outR + done, n);
            beat_ += n * beatsPerFrame_;
            done += n;
        }
        publishVisual();
    }

    const SampleEngine& engine() const { return engine_; }

private:
    struct VisualState {
        int32_t poolCount = 0;
        int32_t frozen = 0;
        SRSlotVisual slots[SR_MAX_SLOTS] {};
        float scopeL[SR_SCOPE_POINTS] {};
        float scopeR[SR_SCOPE_POINTS] {};
    };

    struct Smoothed {
        float value = 0.f, previous = 0.f;
        void step(float target, float a) {
            previous = value;
            value += (target - value) * a;
        }
        void reset(float v) { value = previous = v; }
        float at(float t) const { return previous + (value - previous) * t; }
    };

    float p(SRParam a) const { return params_[a].load(std::memory_order_relaxed); }

    // While the host's transport plays, follow its position. Otherwise run free at the host's tempo,
    // or at the Tempo parameter when there is no host tempo (the standalone app).
    void beginClock() {
        const bool fromHost = host_.hasTempo && host_.tempo > 0.0;
        const double tempo = std::min(std::max(fromHost ? host_.tempo : static_cast<double>(p(SRParamTempo)), 20.0), 400.0);
        beatsPerFrame_ = tempo / (60.0 * sr_);
        barBeats_ = host_.hasPosition && host_.barBeats > 0.0 ? host_.barBeats : 4.0;
        // Without transport state, the position only counts as playing while it moves.
        const bool playing = host_.hasPosition &&
                             (host_.hasTransport ? host_.playing : host_.beat != lastHostBeat_);
        lastHostBeat_ = host_.hasPosition ? host_.beat : -1.0;
        if (playing) {
            beat_ = host_.beat;
            barOrigin_ = host_.downbeat;
        }
        const int grid = std::min(std::max(static_cast<int>(std::lround(p(SRParamGrid))), 0), kGridCount - 1);
        gridBeats_ = kGridBeats[grid] > 0.0 ? kGridBeats[grid] : barBeats_;
        shownTempo_.store(tempo, std::memory_order_relaxed);
        shownBeat_.store(beat_ - barOrigin_, std::memory_order_relaxed);
        shownFromHost_.store(fromHost, std::memory_order_relaxed);
    }

    void processChunk(const float* inL, const float* inR, float* outL, float* outR, int n) {
        const bool freeze = p(SRParamFreeze) >= 0.5f;
        frozen_ = freeze;
        const float mixT = clampf(p(SRParamMix) / 100.f, 0.f, 1.f);
        const float levelT = clampf(p(SRParamSampleLevel) / 100.f, 0.f, 1.f);
        const float dryT = clampf(p(SRParamDryFeed) / 100.f, 0.f, 1.f);
        const float filterT = clampf(p(SRParamFilter) / 100.f, -1.f, 1.f);
        const float preT = clampf(p(SRParamPreDelay), 0.f, 500.f) * 0.001f * sr_;
        const float sizeT = clampf(p(SRParamSize) / 100.f, 0.f, 1.f);

        if (first_) {
            mix_.reset(mixT);
            level_.reset(levelT);
            dry_.reset(dryT);
            filter_.reset(filterT);
            size_.reset(sizeT);
            preCur_ = preT;
            first_ = false;
        } else {
            const float a = 1.f - std::exp(-static_cast<float>(n) / (0.03f * sr_));
            mix_.step(mixT, a);
            level_.step(levelT, a);
            dry_.step(dryT, a);
            filter_.step(filterT, a);
            size_.step(sizeT, a);
        }

        EngineParams ep;
        ep.retention = p(SRParamRetention) / 100.f;
        ep.sensitivityDb = p(SRParamSensitivity);
        ep.match = p(SRParamMatch) / 100.f;
        ep.harmony = p(SRParamHarmony) / 100.f;
        ep.lengthSec = p(SRParamLength);
        ep.attackSec = p(SRParamAttack) * 0.001f;
        ep.density = static_cast<int>(std::lround(p(SRParamDensity)));
        ep.focusX = p(SRParamFocusX);
        ep.focusY = p(SRParamFocusY);
        ep.focusAmount = p(SRParamFocusAmount) / 100.f;
        ep.freeze = freeze;
        ep.quantize = p(SRParamQuantize) >= 0.5f;
        ep.gridBeats = gridBeats_;
        engine_.setParams(ep);

        FDNReverb::Params rp;
        rp.decaySec = p(SRParamDecay);
        rp.size = size_.value;
        rp.tone = p(SRParamTone) / 100.f;
        rp.texture = p(SRParamTexture) / 100.f;
        rp.mod = p(SRParamMod) / 100.f;
        rp.shimmer = p(SRParamShimmer) / 100.f;
        const int interval = std::min(std::max(static_cast<int>(std::lround(p(SRParamInterval))), 0), kIntervalCount - 1);
        rp.shimmerSemitones = kIntervalSemitones[interval];
        rp.sub = p(SRParamSub) / 100.f;
        rp.freeze = freeze;
        reverb_.setParams(rp);

        // Copy input first so processing is safe in place.
        float xl[kChunk], xr[kChunk], vl[kChunk], vr[kChunk], rl[kChunk], rr[kChunk], wl[kChunk], wr[kChunk];
        std::memcpy(xl, inL, sizeof(float) * n);
        std::memcpy(xr, inR, sizeof(float) * n);

        engine_.process(xl, xr, vl, vr, n);

        const float preCoef = 1.f - std::exp(-1.f / (0.05f * sr_));
        for (int i = 0; i < n; ++i) {
            const float t = static_cast<float>(i + 1) / n;
            const float dry = dry_.at(t), level = level_.at(t);
            preL_.write(xl[i] * dry + vl[i] * level);
            preR_.write(xr[i] * dry + vr[i] * level);
            preCur_ += (preT - preCur_) * preCoef;
            rl[i] = preL_.read(preCur_);
            rr[i] = preR_.read(preCur_);
        }

        reverb_.process(rl, rr, wl, wr, n);

        // Bipolar filter: left = low-pass sweep 20 kHz → 200 Hz, right = high-pass 20 Hz → 4 kHz.
        const float f = filter_.value;
        const bool highpass = f > 0.f;
        const float cutoff = std::min(highpass ? 20.f * std::pow(200.f, f) : 20000.f * std::pow(0.01f, -f), 0.45f * sr_);
        const float res = clampf(p(SRParamResonance) / 100.f, 0.f, 1.f);
        const float q = 0.707f + res * 9.3f * std::min(1.f, std::fabs(f) * 5.f);
        filterL_.set(cutoff, q, sr_);
        filterR_.set(cutoff, q, sr_);

        for (int i = 0; i < n; ++i) {
            float lp, hp;
            filterL_.process(wl[i], lp, hp);
            const float yl = highpass ? hp : lp;
            filterR_.process(wr[i], lp, hp);
            const float yr = highpass ? hp : lp;

            scopeL_[scopeW_] = yl;
            scopeR_[scopeW_] = yr;
            scopeW_ = (scopeW_ + 1) & (SR_SCOPE_POINTS - 1);

            const float angle = mix_.at(static_cast<float>(i + 1) / n) * kPi * 0.5f;
            const float dryGain = std::cos(angle), wetGain = std::sin(angle);
            outL[i] = xl[i] * dryGain + yl * wetGain;
            outR[i] = xr[i] * dryGain + yr * wetGain;
        }

        engine_.advanceLife(static_cast<float>(n) / sr_);
    }

    void publishVisual() {
        VisualState& v = visual_.backBuffer();
        v.poolCount = engine_.poolCount();
        v.frozen = frozen_ ? 1 : 0;
        engine_.fillVisual(v.slots);
        const int head = SR_SCOPE_POINTS - scopeW_;
        std::memcpy(v.scopeL, scopeL_ + scopeW_, sizeof(float) * head);
        std::memcpy(v.scopeL + head, scopeL_, sizeof(float) * scopeW_);
        std::memcpy(v.scopeR, scopeR_ + scopeW_, sizeof(float) * head);
        std::memcpy(v.scopeR + head, scopeR_, sizeof(float) * scopeW_);
        visual_.publish();
    }

    static_assert((SR_SCOPE_POINTS & (SR_SCOPE_POINTS - 1)) == 0, "scope ring must be a power of two");

    std::atomic<float> params_[SRParamCount];
    std::atomic<bool> bypass_ {false};

    float sr_ = 48000.f;
    bool first_ = true;
    bool frozen_ = false;
    Smoothed mix_, level_, dry_, filter_, size_;
    float preCur_ = 0.f;

    HostTime host_;
    double beat_ = 0.0, beatsPerFrame_ = 0.0, barOrigin_ = 0.0, barBeats_ = 4.0, gridBeats_ = 0.5;
    double lastHostBeat_ = -1.0;
    std::atomic<double> shownTempo_ {120.0}, shownBeat_ {0.0};
    std::atomic<bool> shownFromHost_ {false};

    SampleEngine engine_;
    FDNReverb reverb_;
    DelayLine preL_, preR_;
    SVF filterL_, filterR_;

    float scopeL_[SR_SCOPE_POINTS] {}, scopeR_[SR_SCOPE_POINTS] {};
    int scopeW_ = 0;
    TripleBuffer<VisualState> visual_;
};

} // namespace sr
