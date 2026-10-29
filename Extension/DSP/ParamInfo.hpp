// Parameter metadata table. Single source of truth for ranges and defaults.
#pragma once

#include "ParamIDs.h"

namespace sr {

enum class Unit { Percent, Seconds, Milliseconds, Decibels, Indexed, Boolean, Bpm, Generic };

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

// Note divisions are in quarter-note beats; a negative value -n means n bars of the current time
// signature.
inline double divisionBeats(double division, double barBeats) { return division < 0.0 ? -division * barBeats : division; }

// Quantize grid.
inline constexpr int kGridCount = 8;
inline constexpr double kGridBeats[kGridCount] = {0.125, 0.25, 1.0 / 3.0, 0.5, 2.0 / 3.0, 1.0, 2.0, -1.0};
inline constexpr const char* kGridNames[kGridCount] = {"1/32", "1/16", "1/8T", "1/8", "1/4T", "1/4", "1/2", "1 Bar"};

// Synced Pre-delay: T = triplet, D = dotted.
inline constexpr int kPreDelayDivCount = 13;
inline constexpr double kPreDelayDivBeats[kPreDelayDivCount] = {
    1.0 / 16, 1.0 / 12, 1.0 / 8, 1.0 / 6, 1.0 / 4, 3.0 / 8, 1.0 / 3, 1.0 / 2, 3.0 / 4, 2.0 / 3, 1.0, 3.0 / 2, 2.0};
inline constexpr const char* kPreDelayDivNames[kPreDelayDivCount] = {
    "1/64", "1/32T", "1/32", "1/16T", "1/16", "1/16D", "1/8T", "1/8", "1/8D", "1/4T", "1/4", "1/4D", "1/2"};
inline constexpr float kMaxPreDelaySec = 3.f;  // a half note at 40 BPM

// Synced Length.
inline constexpr int kLengthDivCount = 7;
inline constexpr double kLengthDivBeats[kLengthDivCount] = {0.5, 1.0, 2.0, -1.0, -2.0, -4.0, -8.0};
inline constexpr const char* kLengthDivNames[kLengthDivCount] = {"1/8", "1/4", "1/2", "1 Bar", "2 Bars", "4 Bars", "8 Bars"};

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
    {SRParamLength,      "length",      "Length",       0.1f,  20.f,   3.f,   Unit::Seconds,      true},
    {SRParamSampleLevel, "samplelevel", "Sample Level", 0.f,   100.f,  80.f,  Unit::Percent,      false},
    {SRParamDensity,     "density",     "Density",      1.f,   8.f,    4.f,   Unit::Generic,      false},
    {SRParamDryFeed,     "dryfeed",     "Dry Feed",     0.f,   100.f,  50.f,  Unit::Percent,      false},
    {SRParamFocusX,      "focusx",      "Focus X",      0.f,   1.f,    0.5f,  Unit::Generic,      false},
    {SRParamFocusY,      "focusy",      "Focus Y",      0.f,   1.f,    0.5f,  Unit::Generic,      false},
    {SRParamFocusAmount, "focus",       "Focus",        0.f,   100.f,  0.f,   Unit::Percent,      false},
    {SRParamSub,         "sub",         "Sub",          0.f,   100.f,  0.f,   Unit::Percent,      false},
    {SRParamAttack,      "attack",      "Attack",       1.f,   2000.f, 100.f, Unit::Milliseconds, true},
    {SRParamQuantize,    "quantize",    "Quantize",     0.f,   1.f,    0.f,   Unit::Boolean,      false},
    {SRParamGrid,        "grid",        "Grid",         0.f,   7.f,    3.f,   Unit::Indexed,      false},
    {SRParamTempo,       "tempo",       "Tempo",        40.f,  240.f,  120.f, Unit::Bpm,          false},
    {SRParamSync,        "sync",        "Sync",         0.f,   1.f,    0.f,   Unit::Boolean,      false},
    {SRParamPreDelayDiv, "predelaydiv", "Pre-delay Sync", 0.f, 12.f,   2.f,   Unit::Indexed,      false},
    {SRParamLengthDiv,   "lengthdiv",   "Length Sync",  0.f,   6.f,    3.f,   Unit::Indexed,      false},
};

// Performance controls: presets leave these alone (Freeze, and the clock, which belongs to the song).
inline constexpr bool presetLeavesAlone(uint64_t address) {
    return address == SRParamFreeze || address == SRParamQuantize || address == SRParamGrid || address == SRParamTempo ||
           address == SRParamSync;
}

} // namespace sr
