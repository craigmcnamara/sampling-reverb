// Parameter metadata table. Single source of truth for ranges and defaults.
#pragma once

#include "ParamIDs.h"

namespace sr {

enum class Unit { Percent, Seconds, Milliseconds, Decibels, Indexed, Boolean, Generic };

struct ParamSpec {
    SRParam address;
    const char* identifier;
    const char* name;
    float min, max, def;
    Unit unit;
    bool exponential;
};

inline constexpr int kIntervalCount = 6;
inline constexpr float kIntervalSemitones[kIntervalCount] = {-12.f, -5.f, 5.f, 7.f, 12.f, 19.f};
inline constexpr const char* kIntervalNames[kIntervalCount] = {"-12", "-5", "+5", "+7", "+12", "+19"};

inline constexpr ParamSpec kParamSpecs[SRParamCount] = {
    {SRParamMix,         "mix",         "Mix",          0.f,   100.f,  50.f,  Unit::Percent,      false},
    {SRParamDecay,       "decay",       "Decay",        0.3f,  30.f,   4.f,   Unit::Seconds,      true},
    {SRParamSize,        "size",        "Size",         0.f,   100.f,  60.f,  Unit::Percent,      false},
    {SRParamPreDelay,    "predelay",    "Pre-delay",    0.f,   500.f,  20.f,  Unit::Milliseconds, false},
    {SRParamTone,        "tone",        "Tone",         0.f,   100.f,  60.f,  Unit::Percent,      false},
    {SRParamTexture,     "texture",     "Texture",      0.f,   100.f,  50.f,  Unit::Percent,      false},
    {SRParamMod,         "mod",         "Mod",          0.f,   100.f,  30.f,  Unit::Percent,      false},
    {SRParamShimmer,     "shimmer",     "Shimmer",      0.f,   100.f,  0.f,   Unit::Percent,      false},
    {SRParamInterval,    "interval",    "Interval",     0.f,   5.f,    4.f,   Unit::Indexed,      false},
    {SRParamFilter,      "filter",      "Filter",      -100.f, 100.f,  0.f,   Unit::Generic,      false},
    {SRParamResonance,   "resonance",   "Resonance",    0.f,   100.f,  20.f,  Unit::Percent,      false},
    {SRParamFreeze,      "freeze",      "Freeze",       0.f,   1.f,    0.f,   Unit::Boolean,      false},
    {SRParamRetention,   "retention",   "Retention",    0.f,   100.f,  50.f,  Unit::Percent,      false},
    {SRParamSensitivity, "sensitivity", "Sensitivity", -60.f, -20.f,  -40.f,  Unit::Decibels,     false},
    {SRParamMatch,       "match",       "Match",        0.f,   100.f,  40.f,  Unit::Percent,      false},
    {SRParamHarmony,     "harmony",     "Harmony",      0.f,   100.f,  30.f,  Unit::Percent,      false},
    {SRParamLength,      "length",      "Length",       0.1f,  2.f,    0.8f,  Unit::Seconds,      false},
    {SRParamSampleLevel, "samplelevel", "Sample Level", 0.f,   100.f,  80.f,  Unit::Percent,      false},
    {SRParamDensity,     "density",     "Density",      1.f,   8.f,    4.f,   Unit::Generic,      false},
    {SRParamDryFeed,     "dryfeed",     "Dry Feed",     0.f,   100.f,  50.f,  Unit::Percent,      false},
    {SRParamFocusX,      "focusx",      "Focus X",      0.f,   1.f,    0.5f,  Unit::Generic,      false},
    {SRParamFocusY,      "focusy",      "Focus Y",      0.f,   1.f,    0.5f,  Unit::Generic,      false},
    {SRParamFocusAmount, "focus",       "Focus",        0.f,   100.f,  0.f,   Unit::Percent,      false},
    {SRParamSub,         "sub",         "Sub",          0.f,   100.f,  0.f,   Unit::Percent,      false},
};

} // namespace sr
