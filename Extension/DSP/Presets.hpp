// Factory presets. Values are listed in SRParam order (see ParamIDs.h).
#pragma once

#include "ParamIDs.h"
#include "ParamInfo.hpp"

namespace sr {

struct Preset {
    const char* name;
    float values[SRParamCount];
};

// "Default" is built from the parameter table so it can never drift from the real defaults.
constexpr Preset makeDefaultPreset() {
    Preset p {"Default", {}};
    for (const auto& spec : kParamSpecs) p.values[spec.address] = spec.def;
    return p;
}

// Presets don't choose the performance controls (see presetLeavesAlone); store their defaults.
constexpr Preset withPerformanceDefaults(Preset p) {
    for (const auto& spec : kParamSpecs)
        if (presetLeavesAlone(spec.address)) p.values[spec.address] = spec.def;
    return p;
}

inline constexpr int kPresetCount = 7;

// clang-format off
inline constexpr Preset kPresets[kPresetCount] = {
    makeDefaultPreset(),
    //                                          Mix  Decay Size Pre  Tone Tex  Mod  Shim Int  Filt Res  Frz  Ret  Sens Match Harm Len   Lvl  Dens Dry  FX    FY    Focus Sub  Atk
    withPerformanceDefaults({"Night Sky",       {55, 12,   85,  40,  45,  80,  30,  35,  4,  -10,  0,   0,   60, -40,  40,   40,  6.0f, 80,  4,   40,  0.5f, 0.5f, 0,    35,  400}}),
    withPerformanceDefaults({"Echo Garden",     {60, 5,    60,  80,  65,  30,  25,  0,   4,   0,   20,  0,   80, -45,  55,   85,  8.0f, 100, 6,   0,   0.5f, 0.5f, 0,    0,   60}}),
    withPerformanceDefaults({"Dark Matter",     {50, 20,   100, 10,  25,  70,  60,  0,   0,  -55,  35,  0,   50, -40,  40,   20,  5.0f, 70,  4,   50,  0.5f, 0.5f, 0,    65,  800}}),
    withPerformanceDefaults({"Glass Choir",     {50, 8,    70,  30,  90,  85,  35,  60,  3,   35,  25,  0,   40, -40,  45,   70,  3.0f, 70,  5,   50,  0.5f, 0.5f, 0,    0,   250}}),
    withPerformanceDefaults({"Short Memory",    {40, 1.5f, 30,  0,   60,  40,  15,  0,   4,   0,   20,  0,   0,  -35,  50,   30,  0.4f, 90,  8,   60,  0.5f, 0.5f, 0,    0,   10}}),
    withPerformanceDefaults({"Infinite Archive",{50, 25,   90,  60,  50,  60,  40,  20,  5,   0,   20,  0,   100, -50, 25,   50,  20.f, 80,  3,   30,  0.5f, 0.5f, 0,    25,  600}}),
};
// clang-format on

} // namespace sr
