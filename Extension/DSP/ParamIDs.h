// Parameter addresses shared by the C++ kernel, the Objective-C++ AU and Swift.
#pragma once

#include <CoreFoundation/CFAvailability.h>
#include <stdint.h>

typedef CF_ENUM(uint64_t, SRParam) {
    // Reverb
    SRParamMix = 0,
    SRParamDecay,
    SRParamSize,
    SRParamPreDelay,
    SRParamTone,
    SRParamTexture,
    SRParamMod,
    SRParamShimmer,
    SRParamInterval,
    SRParamFilter,
    SRParamResonance,
    SRParamFreeze,
    // Sampler
    SRParamRetention,
    SRParamSensitivity,
    SRParamMatch,
    SRParamHarmony,
    SRParamLength,
    SRParamSampleLevel,
    SRParamDensity,
    SRParamDryFeed,
    // X/Y focus
    SRParamFocusX,
    SRParamFocusY,
    SRParamFocusAmount,
    // Added after 0.1: new parameters go at the end so saved states keep their addresses.
    SRParamSub,
    SRParamAttack,
    // Tempo: trigger quantize
    SRParamQuantize,
    SRParamGrid,
    SRParamTempo,

    SRParamCount
};
