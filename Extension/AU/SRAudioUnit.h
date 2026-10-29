#import <AudioToolbox/AudioToolbox.h>

#import "ParamIDs.h"
#import "VisualTypes.h"

NS_ASSUME_NONNULL_BEGIN

/// The Sampling Reverb AUAudioUnit. The render path lives in C++ (see SamplingReverbKernel.hpp).
@interface SRAudioUnit : AUAudioUnit

/// Copies the latest visual snapshot published by the audio thread. Call from the main thread only.
/// Returns the number of samples in the pool.
- (NSInteger)readVisualSlots:(SRSlotVisual *)slots
                   slotCount:(NSInteger)slotCount
                   scopeLeft:(float *_Nullable)scopeLeft
                  scopeRight:(float *_Nullable)scopeRight
                  scopeCount:(NSInteger)scopeCount
                      frozen:(BOOL *_Nullable)frozen;

/// The tempo the plugin is running at (the host's, or the Tempo parameter when the host has none),
/// and the position in the bar in quarter-note beats. Any thread.
- (double)currentTempoFromHost:(BOOL *_Nullable)fromHost beatInBar:(double *_Nullable)beatInBar;

/// Parameter values for factory preset `index`, keyed by parameter address.
- (NSDictionary<NSNumber *, NSNumber *> *)valuesForFactoryPresetAtIndex:(NSInteger)index;

@end

NS_ASSUME_NONNULL_END
