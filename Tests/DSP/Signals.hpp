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
