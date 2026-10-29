// Small real-time-safe DSP building blocks.
#pragma once

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <vector>

namespace sr {

constexpr float kPi = 3.14159265358979f;
constexpr float kTwoPi = 2.f * kPi;

inline float clampf(float x, float lo, float hi) { return std::min(std::max(x, lo), hi); }
inline float lerpf(float a, float b, float t) { return a + (b - a) * t; }
inline float dbToGain(float db) { return std::pow(10.f, db / 20.f); }
inline float gainToDb(float g) { return 20.f * std::log10(std::max(g, 1e-9f)); }

inline uint32_t nextPow2(uint32_t v) {
    uint32_t n = 1;
    while (n < v) n <<= 1;
    return n;
}

// Transparent below |x| = 1, then saturates smoothly towards ±2. Keeps feedback loops bounded.
inline float safeLimit(float x) {
    const float a = std::fabs(x);
    if (a <= 1.f) return x;
    return std::copysign(1.f + std::tanh(a - 1.f), x);
}

struct Rng {
    uint32_t state = 0x9E3779B9u;
    uint32_t next() {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        return state;
    }
    float uniform() { return (next() >> 8) * (1.f / 16777216.f); }
    float bipolar() { return uniform() * 2.f - 1.f; }
};

// Power-of-two circular delay line. read(0) returns the most recently written sample.
struct DelayLine {
    std::vector<float> buf;
    uint32_t mask = 0;
    uint32_t w = 0;

    void allocate(uint32_t maxDelay) {
        const uint32_t n = nextPow2(maxDelay + 4);
        buf.assign(n, 0.f);
        mask = n - 1;
        w = 0;
    }
    void clear() { std::fill(buf.begin(), buf.end(), 0.f); }

    inline void write(float x) {
        buf[w] = x;
        w = (w + 1) & mask;
    }
    inline float readInt(uint32_t d) const { return buf[(w - 1 - d) & mask]; }
    inline float read(float d) const {
        const uint32_t i = static_cast<uint32_t>(d);
        const float f = d - static_cast<float>(i);
        const float a = buf[(w - 1 - i) & mask];
        const float b = buf[(w - 2 - i) & mask];
        return a + (b - a) * f;
    }
};

// Schroeder allpass diffuser.
struct Allpass {
    DelayLine d;
    uint32_t len = 1;
    float g = 0.5f;

    void allocate(uint32_t length) {
        len = std::max<uint32_t>(length, 1);
        d.allocate(len);
    }
    inline float process(float x) {
        const float delayed = d.readInt(len - 1);
        const float v = x + g * delayed;
        d.write(v);
        return delayed - g * v;
    }
};

// Cytomic/TPT state-variable filter.
struct SVF {
    float ic1 = 0.f, ic2 = 0.f;
    float a1 = 1.f, a2 = 0.f, a3 = 0.f, k = 1.f;

    void set(float cutoff, float q, float sampleRate) {
        const float g = std::tan(kPi * cutoff / sampleRate);
        k = 1.f / q;
        a1 = 1.f / (1.f + g * (g + k));
        a2 = g * a1;
        a3 = g * a2;
    }
    void reset() { ic1 = ic2 = 0.f; }
    inline void process(float v0, float& lp, float& hp) {
        const float v3 = v0 - ic2;
        const float v1 = a1 * ic1 + a2 * v3;
        const float v2 = ic2 + a2 * ic1 + a3 * v3;
        ic1 = 2.f * v1 - ic1;
        ic2 = 2.f * v2 - ic2;
        lp = v2;
        hp = v0 - k * v1 - v2;
    }
};

} // namespace sr
