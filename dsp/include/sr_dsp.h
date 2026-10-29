// C interface to the Rust DSP core (dsp/src/ffi.rs).
#pragma once

#include <stdbool.h>
#include <stdint.h>

#include "VisualTypes.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef struct SRKernel SRKernel;

// What the host reports at the start of a render block. Any part may be missing.
typedef struct SRHostTime {
    bool hasTempo;
    double tempo;
    bool hasPosition;
    double beat;      // quarter notes
    double downbeat;  // beat of the current bar's first beat
    double barBeats;  // quarter notes per bar
    bool hasTransport;
    bool playing;
} SRHostTime;

typedef struct SRClockSummary {
    double tempo;
    double beat;  // position in the bar, quarter notes
    bool fromHost;
} SRClockSummary;

typedef enum SRUnit : uint32_t {
    SRUnitPercent = 0,
    SRUnitSeconds,
    SRUnitMilliseconds,
    SRUnitDecibels,
    SRUnitIndexed,
    SRUnitBoolean,
    SRUnitBpm,
    SRUnitGeneric,
} SRUnit;

typedef struct SRParamSpec {
    const char *identifier;
    const char *name;
    float min, max, def;
    uint32_t unit;  // SRUnit
    bool exponential;
} SRParamSpec;

// Lifetime and preparation (main thread, while not rendering).
SRKernel *sr_kernel_new(void);
void sr_kernel_free(SRKernel *k);
void sr_kernel_prepare(const SRKernel *k, double sampleRate);

// Any thread.
void sr_kernel_set_parameter(const SRKernel *k, uint64_t address, float value);
float sr_kernel_get_parameter(const SRKernel *k, uint64_t address);
void sr_kernel_set_bypass(const SRKernel *k, bool bypass);
SRClockSummary sr_kernel_clock(const SRKernel *k);

// Audio thread. Outputs may be the inputs (in place) and inL may equal inR; outL and outR must
// not overlap. host may be NULL.
void sr_kernel_process(const SRKernel *k, const SRHostTime *host, const float *inL, const float *inR,
                       float *outL, float *outR, int32_t frames);

// One UI thread. Returns the pool count.
int32_t sr_kernel_read_visual(const SRKernel *k, SRSlotVisual *slots, int32_t slotCount, float *scopeL,
                              float *scopeR, int32_t scopeCount, bool *frozen);

// Metadata (any thread). Strings are static.
int32_t sr_param_count(void);
SRParamSpec sr_param_spec(uint64_t address);
int32_t sr_param_value_name_count(uint64_t address);
const char *sr_param_value_name(uint64_t address, int32_t index);
int32_t sr_preset_count(void);
const char *sr_preset_name(int32_t index);
float sr_preset_value(int32_t index, uint64_t address);
bool sr_preset_leaves_alone(uint64_t address);

#ifdef __cplusplus
}
#endif
