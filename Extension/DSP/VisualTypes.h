// Plain C types the kernel publishes for the UI. Imported into Swift via the bridging header.
#pragma once

#include <stdint.h>

#define SR_MAX_SLOTS 24
#define SR_SCOPE_POINTS 1024

typedef struct SRSlotVisual {
    int32_t active;
    uint32_t generation;    // increments each time the slot is refilled
    uint32_t triggerCount;  // increments each time the slot is played
    float x;                // brightness, 0…1
    float y;                // chroma (pitched) or loudness (unpitched), 0…1
    float life;             // remaining life / max life, 0…2
    float clarity;          // 0 = noisy, 1 = clearly pitched
    float level;            // current playback level, 0…1
} SRSlotVisual;
