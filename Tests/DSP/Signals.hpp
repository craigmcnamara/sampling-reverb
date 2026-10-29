// Test signal generators and measurements.
#pragma once

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <vector>

namespace sig {

using Buffer = std::vector<float>;

inline size_t frames(float seconds, float sampleRate) { return static_cast<size_t>(seconds * sampleRate); }

inline Buffer silence(float seconds, float sr) { return Buffer(frames(seconds, sr), 0.f); }

// Sine burst with a 5 ms attack and an exponential release, like a plucked note.
inline Buffer toneBurst(float hz, float seconds, float amp, float sr) {
    Buffer b(frames(seconds, sr));
    const float attack = 0.005f * sr;
    for (size_t i = 0; i < b.size(); ++i) {
        const float t = static_cast<float>(i);
        const float env = std::min(1.f, t / attack) * std::exp(-3.f * t / b.size());
        b[i] = amp * env * std::sin(2.f * 3.14159265f * hz * t / sr);
    }
    return b;
}

inline Buffer noiseBurst(float seconds, float amp, float sr, uint32_t seed = 1) {
    Buffer b(frames(seconds, sr));
    const float attack = 0.002f * sr;
    for (size_t i = 0; i < b.size(); ++i) {
        seed = seed * 1664525u + 1013904223u;
        const float n = (seed >> 8) * (2.f / 16777216.f) - 1.f;
        const float t = static_cast<float>(i);
        b[i] = amp * n * std::min(1.f, t / attack) * std::exp(-4.f * t / b.size());
    }
    return b;
}

inline Buffer sine(float hz, float seconds, float amp, float sr) {
    Buffer b(frames(seconds, sr));
    for (size_t i = 0; i < b.size(); ++i) b[i] = amp * std::sin(2.f * 3.14159265f * hz * i / sr);
    return b;
}

// Karplus-Strong plucked string: a reasonable stand-in for an electric/acoustic guitar note.
inline Buffer pluck(float hz, float seconds, float amp, float sr, uint32_t seed = 1) {
    Buffer b(frames(seconds, sr));
    const size_t period = std::max<size_t>(2, static_cast<size_t>(sr / hz));
    std::vector<float> line(period);
    for (auto& v : line) {
        seed = seed * 1664525u + 1013904223u;
        v = (seed >> 8) * (2.f / 16777216.f) - 1.f;
    }
    size_t idx = 0;
    for (size_t i = 0; i < b.size(); ++i) {
        const size_t next = (idx + 1) % period;
        const float out = line[idx];
        line[idx] = 0.996f * 0.5f * (line[idx] + line[next]);
        idx = next;
        b[i] = amp * out;
    }
    return b;
}

// A strummed open E-minor-ish chord (E2 B2 E3 G3 B3 E4), 12 ms between strings.
inline Buffer strum(float seconds, float amp, float sr) {
    const float notes[] = {82.41f, 123.47f, 164.81f, 196.f, 246.94f, 329.63f};
    Buffer b(frames(seconds, sr), 0.f);
    for (int k = 0; k < 6; ++k) {
        const Buffer s = pluck(notes[k], seconds, amp / 3.f, sr, 11 + k);
        const size_t offset = frames(0.012f * k, sr);
        for (size_t i = 0; i + offset < b.size(); ++i) b[i + offset] += s[i];
    }
    return b;
}

inline void append(Buffer& dst, const Buffer& src) { dst.insert(dst.end(), src.begin(), src.end()); }

inline float rms(const float* x, size_t n) {
    double s = 0.0;
    for (size_t i = 0; i < n; ++i) s += double(x[i]) * x[i];
    return n ? static_cast<float>(std::sqrt(s / n)) : 0.f;
}
inline float rms(const Buffer& b, size_t from, size_t to) {
    to = std::min(to, b.size());
    return from < to ? rms(b.data() + from, to - from) : 0.f;
}
inline float peak(const Buffer& b) {
    float p = 0.f;
    for (float v : b) p = std::max(p, std::fabs(v));
    return p;
}
inline bool allFinite(const Buffer& b) {
    for (float v : b)
        if (!std::isfinite(v)) return false;
    return true;
}
inline float db(float g) { return 20.f * std::log10(std::max(g, 1e-12f)); }

// Level of one frequency component over [from, to) (Goertzel, Hann window), as an amplitude.
inline float toneLevel(const Buffer& b, size_t from, size_t to, float hz, float sr) {
    to = std::min(to, b.size());
    if (to <= from) return 0.f;
    const size_t n = to - from;
    const double w = 2.0 * 3.14159265358979 * hz / sr, coeff = 2.0 * std::cos(w);
    double s1 = 0.0, s2 = 0.0, norm = 0.0;
    for (size_t i = 0; i < n; ++i) {
        const double hann = 0.5 - 0.5 * std::cos(2.0 * 3.14159265358979 * i / n);
        const double s0 = b[from + i] * hann + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
        norm += hann;
    }
    const double power = s1 * s1 + s2 * s2 - coeff * s1 * s2;
    return static_cast<float>(2.0 * std::sqrt(std::max(power, 0.0)) / norm);
}

// Energy (dB) in the band [lo, hi] Hz over [from, end): two cascaded 2-pole high-passes at lo,
// then two cascaded 2-pole low-passes at hi.
float bandEnergyDb(const Buffer& x, size_t from, float lo, float hi, float sr);

// Frequency estimate from rising zero crossings.
inline float zeroCrossingHz(const Buffer& b, size_t from, size_t to, float sr) {
    int crossings = 0;
    size_t first = 0, last = 0;
    for (size_t i = from + 1; i < to && i < b.size(); ++i) {
        if (b[i - 1] < 0.f && b[i] >= 0.f) {
            if (crossings == 0) first = i;
            last = i;
            ++crossings;
        }
    }
    return crossings > 1 ? (crossings - 1) * sr / static_cast<float>(last - first) : 0.f;
}

} // namespace sig

#include "DSPUtil.hpp"

inline float sig::bandEnergyDb(const Buffer& x, size_t from, float lo, float hi, float sr) {
    sr::SVF h1, h2, l1, l2;
    h1.set(lo, 0.707f, sr);
    h2.set(lo, 0.707f, sr);
    l1.set(hi, 0.707f, sr);
    l2.set(hi, 0.707f, sr);
    double e = 0.0;
    for (size_t i = 0; i < x.size(); ++i) {
        float lp, hp, a, b, c, d, f, g;
        h1.process(x[i], lp, hp);
        h2.process(hp, a, b);
        l1.process(b, c, d);
        l2.process(c, f, g);
        if (i >= from) e += double(f) * f;
    }
    return static_cast<float>(10.0 * std::log10(e + 1e-20));
}
