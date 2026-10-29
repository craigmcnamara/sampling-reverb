#import "SRAudioUnit.h"

#import <AVFoundation/AVFoundation.h>

#include <memory>
#include <vector>

#include "sr_dsp.h"

namespace {

// Everything the render block touches, owned by the AU and captured by raw pointer
// so the block never retains (or messages) the Objective-C object.
struct RenderState {
    // The Rust DSP core (dsp/), through its C ABI.
    SRKernel* const kernel = sr_kernel_new();
    std::vector<float> inL, inR, scratchR;
    AudioBufferList* inputList = nullptr;
    AUAudioFrameCount maxFrames = 0;
    // Cached when render resources are allocated: reading the AU's properties isn't real-time safe.
    AUHostMusicalContextBlock musicalContext = nil;
    AUHostTransportStateBlock transportState = nil;

    SRHostTime hostTime() const {
        SRHostTime t = {false, 120.0, false, 0.0, 0.0, 4.0, false, false};
        if (musicalContext) {
            double tempo = 0, numerator = 4, beat = 0, downbeat = 0;
            NSInteger denominator = 4, offsetToNextBeat = 0;
            if (musicalContext(&tempo, &numerator, &denominator, &beat, &offsetToNextBeat, &downbeat)) {
                t.hasTempo = tempo > 0;
                t.tempo = tempo;
                t.hasPosition = true;
                t.beat = beat;
                t.downbeat = downbeat;
                if (numerator > 0 && denominator > 0) t.barBeats = numerator * 4.0 / denominator;
            }
        }
        if (transportState) {
            AUHostTransportStateFlags flags = 0;
            double position = 0, cycleStart = 0, cycleEnd = 0;
            if (transportState(&flags, &position, &cycleStart, &cycleEnd)) {
                t.hasTransport = true;
                t.playing = (flags & AUHostTransportStateMoving) != 0;
            }
        }
        return t;
    }

    ~RenderState() {
        free(inputList);
        sr_kernel_free(kernel);
    }

    void allocate(AUAudioFrameCount frames) {
        maxFrames = frames;
        inL.assign(frames, 0.f);
        inR.assign(frames, 0.f);
        scratchR.assign(frames, 0.f);
        free(inputList);
        inputList = static_cast<AudioBufferList*>(calloc(1, sizeof(AudioBufferList) + sizeof(AudioBuffer)));
    }

    AudioBufferList* prepareInput(UInt32 channels, AUAudioFrameCount frames) {
        inputList->mNumberBuffers = channels;
        float* data[2] = {inL.data(), inR.data()};
        for (UInt32 c = 0; c < channels; ++c) {
            inputList->mBuffers[c].mNumberChannels = 1;
            inputList->mBuffers[c].mData = data[c];
            inputList->mBuffers[c].mDataByteSize = frames * sizeof(float);
        }
        return inputList;
    }
};

AudioUnitParameterUnit unitFor(uint32_t unit) {
    switch (unit) {
        case SRUnitPercent: return kAudioUnitParameterUnit_Percent;
        case SRUnitSeconds: return kAudioUnitParameterUnit_Seconds;
        case SRUnitMilliseconds: return kAudioUnitParameterUnit_Milliseconds;
        case SRUnitDecibels: return kAudioUnitParameterUnit_Decibels;
        case SRUnitIndexed: return kAudioUnitParameterUnit_Indexed;
        case SRUnitBoolean: return kAudioUnitParameterUnit_Boolean;
        case SRUnitBpm: return kAudioUnitParameterUnit_BPM;
        case SRUnitGeneric: return kAudioUnitParameterUnit_Generic;
    }
    return kAudioUnitParameterUnit_Generic;
}

} // namespace

@implementation SRAudioUnit {
    std::unique_ptr<RenderState> _state;
    AUAudioUnitBus* _inputBus;
    AUAudioUnitBus* _outputBus;
    AUAudioUnitBusArray* _inputBusArray;
    AUAudioUnitBusArray* _outputBusArray;
    AUParameterTree* _parameterTree;
    NSArray<AUAudioUnitPreset*>* _factoryPresets;
    AUAudioUnitPreset* _currentPreset;
}

- (instancetype)initWithComponentDescription:(AudioComponentDescription)componentDescription
                                     options:(AudioComponentInstantiationOptions)options
                                       error:(NSError**)outError {
    self = [super initWithComponentDescription:componentDescription options:options error:outError];
    if (!self) return nil;

    _state = std::make_unique<RenderState>();

    AVAudioFormat* format = [[AVAudioFormat alloc] initStandardFormatWithSampleRate:48000 channels:2];
    _inputBus = [[AUAudioUnitBus alloc] initWithFormat:format error:outError];
    _outputBus = [[AUAudioUnitBus alloc] initWithFormat:format error:outError];
    if (!_inputBus || !_outputBus) return nil;
    _inputBus.maximumChannelCount = 2;
    _outputBus.maximumChannelCount = 2;
    _inputBusArray = [[AUAudioUnitBusArray alloc] initWithAudioUnit:self busType:AUAudioUnitBusTypeInput busses:@[ _inputBus ]];
    _outputBusArray = [[AUAudioUnitBusArray alloc] initWithAudioUnit:self busType:AUAudioUnitBusTypeOutput busses:@[ _outputBus ]];

    [self buildParameterTree];
    [self buildFactoryPresets];
    self.maximumFramesToRender = 4096;
    return self;
}

- (void)buildParameterTree {
    NSMutableArray<AUParameter*>* params = [NSMutableArray array];
    for (uint64_t address = 0; address < SRParamCount; ++address) {
        const SRParamSpec spec = sr_param_spec(address);
        NSArray<NSString*>* valueStrings = nil;
        if (const int32_t count = sr_param_value_name_count(address)) {
            NSMutableArray* names = [NSMutableArray array];
            for (int32_t i = 0; i < count; ++i) [names addObject:@(sr_param_value_name(address, i))];
            valueStrings = names;
        }
        AudioUnitParameterOptions flags = kAudioUnitParameterFlag_IsReadable | kAudioUnitParameterFlag_IsWritable;
        if (spec.exponential) flags |= kAudioUnitParameterFlag_DisplayExponential;
        AUParameter* param = [AUParameterTree createParameterWithIdentifier:@(spec.identifier)
                                                                       name:@(spec.name)
                                                                    address:address
                                                                        min:spec.min
                                                                        max:spec.max
                                                                       unit:unitFor(spec.unit)
                                                                   unitName:nil
                                                                      flags:flags
                                                               valueStrings:valueStrings
                                                        dependentParameters:nil];
        param.value = spec.def;
        [params addObject:param];
    }
    _parameterTree = [AUParameterTree createTreeWithChildren:params];

    SRKernel* kernel = _state->kernel;
    _parameterTree.implementorValueObserver = ^(AUParameter* param, AUValue value) {
        sr_kernel_set_parameter(kernel, param.address, value);
    };
    _parameterTree.implementorValueProvider = ^AUValue(AUParameter* param) {
        return sr_kernel_get_parameter(kernel, param.address);
    };
    _parameterTree.implementorStringFromValueCallback = ^NSString*(AUParameter* param, const AUValue* valuePtr) {
        const AUValue value = valuePtr ? *valuePtr : param.value;
        if (param.valueStrings) {
            const NSInteger index = MIN(MAX(lround(value), 0), (long)param.valueStrings.count - 1);
            return param.valueStrings[index];
        }
        switch (param.unit) {
            case kAudioUnitParameterUnit_Percent: return [NSString stringWithFormat:@"%.0f%%", value];
            case kAudioUnitParameterUnit_Seconds: return [NSString stringWithFormat:@"%.2f s", value];
            case kAudioUnitParameterUnit_Milliseconds: return [NSString stringWithFormat:@"%.0f ms", value];
            case kAudioUnitParameterUnit_Decibels: return [NSString stringWithFormat:@"%.0f dB", value];
            case kAudioUnitParameterUnit_Boolean: return value >= 0.5f ? @"On" : @"Off";
            case kAudioUnitParameterUnit_BPM: return [NSString stringWithFormat:@"%.1f BPM", value];
            default: break;
        }
        if (param.address == SRParamDensity) return [NSString stringWithFormat:@"%ld", lround(value)];
        if (param.address == SRParamFilter) {
            if (fabsf(value) < 1.f) return @"Off";
            return [NSString stringWithFormat:@"%@ %.0f", value < 0 ? @"LP" : @"HP", fabsf(value)];
        }
        return [NSString stringWithFormat:@"%.2f", value];
    };
}

- (void)buildFactoryPresets {
    NSMutableArray<AUAudioUnitPreset*>* presets = [NSMutableArray array];
    for (int32_t i = 0; i < sr_preset_count(); ++i) {
        AUAudioUnitPreset* preset = [AUAudioUnitPreset new];
        preset.number = i;
        preset.name = @(sr_preset_name(i));
        [presets addObject:preset];
    }
    _factoryPresets = presets;
    _currentPreset = presets.firstObject;  // the tree starts at the defaults, which is preset 0
}

#pragma mark - Presets

- (NSArray<AUAudioUnitPreset*>*)factoryPresets { return _factoryPresets; }
- (AUAudioUnitPreset*)currentPreset { return _currentPreset; }

- (void)setCurrentPreset:(AUAudioUnitPreset*)preset {
    if (preset && preset.number >= 0 && preset.number < sr_preset_count()) {
        // Setting values through the tree notifies the kernel, the UI and the host (automation).
        // Freeze and the clock are performance controls, so presets leave them alone.
        const int32_t index = (int32_t)preset.number;
        for (AUParameter* param in _parameterTree.allParameters) {
            if (param.address < SRParamCount && !sr_preset_leaves_alone(param.address))
                param.value = sr_preset_value(index, param.address);
        }
        _currentPreset = _factoryPresets[preset.number];
    } else {
        _currentPreset = preset;
    }
}

- (NSDictionary<NSNumber*, NSNumber*>*)valuesForFactoryPresetAtIndex:(NSInteger)index {
    if (index < 0 || index >= sr_preset_count()) return @{};
    NSMutableDictionary* values = [NSMutableDictionary dictionary];
    for (uint64_t p = 0; p < SRParamCount; ++p) values[@(p)] = @(sr_preset_value((int32_t)index, p));
    return values;
}

#pragma mark - AUAudioUnit overrides

- (AUParameterTree*)parameterTree { return _parameterTree; }
- (AUAudioUnitBusArray*)inputBusses { return _inputBusArray; }
- (AUAudioUnitBusArray*)outputBusses { return _outputBusArray; }
- (NSArray<NSNumber*>*)channelCapabilities { return @[ @1, @1, @2, @2 ]; }
- (BOOL)canProcessInPlace { return YES; }
- (NSTimeInterval)tailTime { return 30.0; }

- (void)setShouldBypassEffect:(BOOL)shouldBypassEffect {
    [super setShouldBypassEffect:shouldBypassEffect];
    sr_kernel_set_bypass(_state->kernel, shouldBypassEffect);
}

- (BOOL)allocateRenderResourcesAndReturnError:(NSError**)outError {
    if (![super allocateRenderResourcesAndReturnError:outError]) return NO;
    if (_inputBus.format.channelCount != _outputBus.format.channelCount) {
        if (outError) *outError = [NSError errorWithDomain:NSOSStatusErrorDomain code:kAudioUnitErr_FormatNotSupported userInfo:nil];
        self.renderResourcesAllocated = NO;
        return NO;
    }
    _state->allocate(self.maximumFramesToRender);
    _state->musicalContext = self.musicalContextBlock;
    _state->transportState = self.transportStateBlock;
    sr_kernel_prepare(_state->kernel, _outputBus.format.sampleRate);
    return YES;
}

- (AUInternalRenderBlock)internalRenderBlock {
    RenderState* state = _state.get();
    return ^AUAudioUnitStatus(AudioUnitRenderActionFlags* actionFlags, const AudioTimeStamp* timestamp,
                              AUAudioFrameCount frameCount, NSInteger outputBusNumber, AudioBufferList* outputData,
                              const AURenderEvent* realtimeEventListHead, AURenderPullInputBlock pullInputBlock) {
        (void)actionFlags;
        (void)outputBusNumber;
        if (frameCount > state->maxFrames) return kAudioUnitErr_TooManyFramesToProcess;
        if (!pullInputBlock) return kAudioUnitErr_NoConnection;

        for (const AURenderEvent* e = realtimeEventListHead; e; e = e->head.next) {
            if (e->head.eventType == AURenderEventParameter || e->head.eventType == AURenderEventParameterRamp)
                sr_kernel_set_parameter(state->kernel, e->parameter.parameterAddress, e->parameter.value);
        }

        const UInt32 channels = MIN(outputData->mNumberBuffers, 2u);
        AudioBufferList* input = state->prepareInput(channels, frameCount);
        AudioUnitRenderActionFlags pullFlags = 0;
        const AUAudioUnitStatus err = pullInputBlock(&pullFlags, timestamp, frameCount, 0, input);
        if (err != noErr) return err;

        for (UInt32 c = 0; c < channels; ++c) {
            if (outputData->mBuffers[c].mData == nullptr) outputData->mBuffers[c].mData = input->mBuffers[c].mData;
            outputData->mBuffers[c].mDataByteSize = frameCount * sizeof(float);
        }
        const float* inL = static_cast<const float*>(input->mBuffers[0].mData);
        const float* inR = channels > 1 ? static_cast<const float*>(input->mBuffers[1].mData) : inL;
        float* outL = static_cast<float*>(outputData->mBuffers[0].mData);
        float* outR = channels > 1 ? static_cast<float*>(outputData->mBuffers[1].mData) : state->scratchR.data();
        const SRHostTime host = state->hostTime();
        sr_kernel_process(state->kernel, &host, inL, inR, outL, outR, static_cast<int32_t>(frameCount));
        return noErr;
    };
}

#pragma mark - Visuals

- (double)currentTempoFromHost:(BOOL*)fromHost beatInBar:(double*)beatInBar {
    const SRClockSummary clock = sr_kernel_clock(_state->kernel);
    if (fromHost) *fromHost = clock.fromHost;
    if (beatInBar) *beatInBar = clock.beat;
    return clock.tempo;
}

- (NSInteger)readVisualSlots:(SRSlotVisual*)slots
                   slotCount:(NSInteger)slotCount
                   scopeLeft:(float*)scopeLeft
                  scopeRight:(float*)scopeRight
                  scopeCount:(NSInteger)scopeCount
                      frozen:(BOOL*)frozen {
    bool isFrozen = false;
    const int32_t pool = sr_kernel_read_visual(_state->kernel, slots, (int32_t)slotCount, scopeLeft, scopeRight,
                                               (int32_t)scopeCount, &isFrozen);
    if (frozen) *frozen = isFrozen;
    return pool;
}

@end
