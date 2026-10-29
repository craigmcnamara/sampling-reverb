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

/// Parameter values for factory preset `index`, keyed by parameter address.
- (NSDictionary<NSNumber *, NSNumber *> *)valuesForFactoryPresetAtIndex:(NSInteger)index;

@end

NS_ASSUME_NONNULL_END
