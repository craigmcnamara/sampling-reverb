# Sampling Reverb — Design

A macOS Audio Unit (AUv3) effect that also runs as a standalone app. As you play, it
records short phrases into a sample pool. When you later play something that sounds
similar or harmonically complementary, it plays back one of those samples into a lush
modulated, shimmering reverb. Samples that keep getting played stay in the pool longer.
Samples that go unplayed fade out and are discarded. The **Retention** control sets how
long that takes.

The UI is a phosphor-oscilloscope X/Y field. Each sample in the pool is a glowing point
in a timbre map. When a sample plays, its point bursts into particles with a bloom glow.
The controls are inspired by the Strymon NightSky.

---

## 1. Platform and technology

| Concern            | Choice                                                         |
|--------------------|----------------------------------------------------------------|
| Plugin format      | AUv3 app extension (`aufx` effect), macOS 13+                  |
| Standalone         | The containing SwiftUI app hosts the same AU via AVAudioEngine |
| Ableton            | Live finds the AU through the system Audio Unit registry      |
| DSP                | C++17, header-only, real-time safe (no allocation/locks in render) |
| AU class           | Thin Objective-C++ `AUAudioUnit` subclass (owns the render block) |
| UI                 | SwiftUI (controls) + Metal (`MTKView`) for scope, particles, bloom |
| Project generation | XcodeGen (`project.yml`). The `.xcodeproj` is generated, not committed |
| Build              | `xcodegen generate && xcodebuild` (see README)                  |

**Why Objective-C++ for the AU class?** The render block runs on the real-time audio
thread. Swift can't guarantee it won't allocate memory or take locks there, so the block
is built in Objective-C++ and calls straight into the C++ kernel. Everything that isn't
real-time (UI, hosting, parameter observation) is Swift.

**Process model.** On macOS an AUv3 runs in its extension process. The AU instance and
its view controller live in that process, so the Metal view reads visualization state
straight from the kernel through a lock-free triple buffer. No IPC is needed. This holds
both in Ableton and in the standalone app.

---

## 2. Signal flow

```
            ┌──────────────────────────────────────────────────────────────┐
 in L/R ──┬─┤ Analyzer: onset detect → feature window → (match | capture)  │
          │ └──────────┬───────────────────────────────┬───────────────────┘
          │            │ trigger(slot)                  │ capture(segment)
          │            ▼                                ▼
          │     ┌──────────────┐                 ┌──────────────┐
          │     │ Voice bank   │◀── reads ──────│ Sample pool  │ (retention / lifetime)
          │     │ (8 voices)   │                 │ (24 slots)   │
          │     └──────┬───────┘                 └──────────────┘
          │            │ × Sample Level
          │            ▼
          ├──× Dry Feed ──(+)──▶ Pre-delay ─▶ Diffuser (Texture) ─▶ FDN reverb ─▶ Filter ─┐
          │                                                ▲   │                          │
          │                                                └ Shimmer (pitch shift) ◀┘     │
          │                                                                               ▼
          └───────────────────────────────── dry ──────────────────────▶ Mix ──▶ out L/R
```

- **Dry Feed** sets how much of the live input reaches the reverb. At 0 the reverb hears
  only the sampled voices, giving a pure "sampling reverb" effect.
- **Freeze** sets the FDN feedback to 1, mutes new reverb input and pauses
  capture, but matched samples can still trigger into the frozen cloud.

---

## 3. Sample accumulation and matching

### 3.1 Onset detection
- A fast envelope (1 ms attack, 10 ms release) is compared with a slow envelope (150 ms).
  An onset fires when `fast > slow × ratio` and `fast > threshold`. The threshold comes
  from **Sensitivity** (−60 to −20 dBFS). The ratio is fixed at about 2 (+6 dB).
- After an onset, the detector holds off for 80 ms so one note doesn't fire twice.

### 3.2 Features
After the onset, the analyzer waits 2048 samples (about 43 ms at 48 kHz). It then
computes features from that window, read out of the input history ring:

| Feature     | Method                                                        | Range  |
|-------------|---------------------------------------------------------------|--------|
| Brightness  | Energy ratio after a one-pole high-pass (~1.5 kHz) vs. total energy | 0…1    |
| Pitch       | Normalized autocorrelation on a 4× decimated window, 60 Hz–1.2 kHz | MIDI note, or none |
| Clarity     | Peak of the normalized autocorrelation (how pitched the sound is) | 0…1    |
| Loudness    | RMS in dBFS                                                   | dB     |

**Map position** (the point in the X/Y view):
- **X** = brightness (dark on the left, bright on the right).
- **Y** = pitch class (chroma, 0…11) laid out bottom to top. For unpitched sounds
  (clarity < 0.5), Y is loudness instead (−60 to 0 dBFS).

### 3.3 Matching ("similar or complementary")
The distance between the current onset `a` and a pooled sample `b` is:

```
d = w_t · |bright_a − bright_b|                    (timbre)
  + w_p · harmonicCost(interval(a,b), Harmony)    (pitch; only if both are pitched)
  + w_c · |clarity_a − clarity_b|                  (pitched vs. noisy)
  + focusAmount · dist(pos_b, focusXY)             (user bias from the X/Y crosshair)
```

`harmonicCost` interpolates between two tables:
- **Similar** (Harmony = 0): unison or octave costs 0, and everything else costs about 1.
- **Complementary** (Harmony = 1): fifths and fourths cost 0.05, major and minor
  thirds/sixths cost 0.15, unison 0.3, and 2nds, 7ths and the tritone cost 1.

The best match with `d < Match` triggers playback. When there's no match, the onset
counts as novel and is captured. One onset never both triggers and captures, so the pool
stays diverse.

### 3.4 Capture
- The input history ring holds `maxLength + preroll` frames of stereo input.
- A capture starts 10 ms before the onset (preroll) and runs for **Length** (0.1 s to 2 s).
  Once the ring has filled, the segment is copied into a free slot in one memcpy. A short
  fade-in and fade-out are applied to avoid clicks.
- If the pool is full, the slot with the lowest remaining lifetime is reused, as long as
  it isn't playing. If every slot is playing, the capture is dropped.

### 3.5 Lifetime and Retention
Each slot has `life` (seconds) and `maxLife`:

```
maxLife = Retention mapped exponentially to 3 s … 300 s   (100% = ∞, never discard)
on capture:  life = maxLife
on trigger:  life = min(life + 0.5·maxLife, 2·maxLife)    // played samples live longer
every block: life −= blockDuration                        // unless Retention = ∞ or Freeze
life ≤ 0 and not playing → slot discarded
```

In the UI, point brightness and size follow `life / maxLife`, so samples visibly fade as
they near discard.

### 3.6 Voices
- There are 8 voices. **Density** (1–8) caps how many play at once. When a voice is
  needed and none is free, the oldest one is released.
- Each voice has a 5 ms attack, plays the sample at its original rate and fades out over
  the last 30 ms. Each voice also gets a slight random pan spread.
- A slot that is already playing can't be retriggered within 100 ms.

---

## 4. Reverb engine

- **Pre-delay**: 0–500 ms.
- **Diffuser (Texture)**: four series allpasses per channel. Texture sets their gain
  (0–0.75): low Texture keeps the echoes distinct, high Texture smears them into a dense
  wash.
- **FDN**: 8 delay lines mixed through a Hadamard matrix. Base lengths are mutually prime
  (about 30–100 ms) and scaled by **Size** (0.25×–2×). Feedback gain per line is
  `g = 10^(−3·len / (RT60·fs))`, with RT60 from **Decay** (0.3 s to 30 s,
  exponential).
- **Modulation**: each line's delay is swept by an LFO (sine, phases spread across the
  lines). **Mod** sets depth (0–3 ms); the rate is fixed at about 0.1–0.9 Hz per line,
  so more Mod makes it more chorused.
- **Tone**: a one-pole low-pass inside the feedback loop (damping), from about 1.5 kHz
  to 18 kHz.
- **Shimmer** (inside the feedback loop): after the Hadamard mix, lines 0–3 each go
  through a two-grain crossfading pitch shifter. **Shimmer** sets how much of each line's
  recirculating signal is *replaced* by its shifted copy, up to 70%. **Interval** picks the
  shift: −12, −5, +5, +7, +12 or +19 semitones.
  - Because the shifted copy replaces part of the signal instead of being added on top, and
    a crossfading shifter never adds gain, the loop can never exceed the decay gain. Each
    pass shifts again, so the cascade builds into the classic shimmer.
  - An earlier version fed shifted output back into the reverb *input*. That pushed the
    loop gain above 1 and the reverb ran away. A test now guards against that.
- **Sub**: the same in-loop technique on lines 4–7, always one octave down and up to 90%
  replacement. A 35 Hz high-pass on the sub path stops repeated octave-down passes from
  piling up inaudible energy. Sub and Shimmer are independent and can be used together.
- **Filter** (post-reverb, like NightSky's filter): one bipolar knob. Turning left of
  center gives a low-pass sweep (20 kHz → 200 Hz), right of center a high-pass sweep
  (20 Hz → 4 kHz), and center is off. **Resonance** sets the Q.
- **Mix**: an equal-power dry/wet crossfade.

---

## 5. Parameters

| ID | Name          | Range              | Default | Section |
|----|---------------|--------------------|---------|---------|
| 0  | Mix           | 0–100 %            | 50      | Reverb  |
| 1  | Decay         | 0.3–30 s           | 4       | Reverb  |
| 2  | Size          | 0–100 %            | 60      | Reverb  |
| 3  | Pre-delay     | 0–500 ms           | 20      | Reverb  |
| 4  | Tone          | 0–100 %            | 60      | Reverb  |
| 5  | Texture       | 0–100 %            | 50      | Reverb  |
| 6  | Mod           | 0–100 %            | 30      | Reverb  |
| 7  | Shimmer       | 0–100 %            | 0       | Reverb  |
| 8  | Interval      | −12/−5/+5/+7/+12/+19 | +12   | Reverb  |
| 9  | Filter        | −100…+100          | 0       | Filter  |
| 10 | Resonance     | 0–100 %            | 20      | Filter  |
| 11 | Freeze        | off/on             | off     | Reverb  |
| 12 | Retention     | 0–100 % (100 = ∞)  | 50      | Sampler |
| 13 | Sensitivity   | −60…−20 dBFS       | −40     | Sampler |
| 14 | Match         | 0–100 %            | 40      | Sampler |
| 15 | Harmony       | 0–100 %            | 30      | Sampler |
| 16 | Length        | 0.1–2 s            | 0.8     | Sampler |
| 17 | Sample Level  | 0–100 %            | 80      | Sampler |
| 18 | Density       | 1–8                | 4       | Sampler |
| 19 | Dry Feed      | 0–100 %            | 50      | Sampler |
| 20 | Focus X       | 0–1                | 0.5     | X/Y     |
| 21 | Focus Y       | 0–1                | 0.5     | X/Y     |
| 22 | Focus         | 0–100 %            | 0       | X/Y     |
| 23 | Sub           | 0–100 %            | 0       | Reverb  |

New parameters are always added at the end, so the addresses in saved Live sets stay valid.

### Factory presets
There are seven AU factory presets, defined in `Extension/DSP/Presets.hpp`. They appear as the
bottom toolbar in the plugin UI and in the host's preset menu.

- **Default** is always first. It's built from the parameter table at compile time, so it
  always matches the true defaults, which is one click back from extreme settings.
- The others are Night Sky, Echo Garden, Dark Matter (uses Sub), Glass Choir, Short Memory
  and Infinite Archive.
- Choosing a preset sets every parameter through the parameter tree, so the host sees and
  can automate the change. Freeze is a performance control, so presets leave it alone.
- A "•" next to the selected preset means a knob has moved since it was chosen.

All parameters are automatable in Live. Continuous parameters are smoothed with a
one-pole filter (about 20 ms) on the audio thread.

---

## 6. User interface

```
┌─────────────────────────────────────────────────────────────────────┐
│  SAMPLING REVERB                                   pool 7/24  ❄︎     │
│ ┌─────────────────────────────────────────────┐  ┌───────────────┐  │
│ │ · · · · · · · · · ·  graticule · · · · · ·  │  │ RETENTION     │  │
│ │        ✶  (playing: particles + bloom)      │  │ SENSITIVITY   │  │
│ │   •          •      ~~ L/R Lissajous ~~     │  │ MATCH         │  │
│ │        •          ┼ focus crosshair         │  │ HARMONY       │  │
│ │  dark ─────────── brightness ─────── bright │  │ LENGTH        │  │
│ └─────────────────────────────────────────────┘  │ LEVEL DENSITY │  │
│  MIX DECAY SIZE PRE TONE TEXTURE MOD SHIMMER INT SUB FILTER RES  ❄︎  │
│ [DEFAULT] [NIGHT SKY] [ECHO GARDEN] [DARK MATTER] [GLASS CHOIR] …    │
└─────────────────────────────────────────────────────────────────────┘
```

### 6.1 Oscilloscope field (Metal)
- **Look**: green-cyan phosphor on near-black, with a faint graticule (10×8 grid and
  center crosshair ticks).
- **Persistence**: an HDR (`rgba16Float`) accumulation texture. Each frame it's
  multiplied by about 0.88 (phosphor decay), then the new geometry is drawn on top with
  additive blending.
- **Trace**: the last ~1024 wet output frames, drawn as a Lissajous curve (X = L,
  Y = R) at low intensity, like an oscilloscope in XY mode.
- **Sample points**: one sprite per slot, placed at (brightness, chroma). Size and
  intensity follow the slot's remaining life. Hue shifts with clarity (pitched = cyan,
  noisy = amber).
- **Playback animation**: when a slot triggers, it emits a burst of 40–80 particles with
  outward velocity, drag, slight upward drift and a 0.6–1.5 s life. While it plays, it
  keeps emitting at a rate set by voice amplitude, and its core flares.
- **Bloom**: the bright pass is downsampled to half resolution, blurred with a separable
  9-tap Gaussian (two iterations), then added back over the accumulation with a
  vignette.
- **Interaction**: dragging sets Focus X/Y, shown as a crosshair. The scroll wheel
  adjusts Focus amount, which also sets the size of the ring around the crosshair.

### 6.2 Controls (SwiftUI)
Rotary knobs: vertical drag, Shift for fine adjustment, double-click to reset. Each knob
binds to an `AUParameter`, and parameter observers keep the UI in sync with host
automation. Freeze is a latching footswitch-style button.

### 6.3 Visual state hand-off
Once per render block, the kernel publishes `VisualState` into a lock-free triple
buffer:
- the pool count and Freeze state
- per slot: active, x, y, life ratio, clarity, voice level and a trigger counter
- the scope ring

The Metal view reads the latest copy at display rate. A new burst starts whenever a
slot's trigger counter changes.

---

## 7. Standalone app

- A SwiftUI window hosts the AU's view (`requestViewController`) and has a small
  transport bar with the source (Audio Input or a looping audio file), Open File… and
  Bypass.
- An `AVAudioEngine` graph runs source → AU effect → main mixer → output.
- It asks for microphone permission (`NSMicrophoneUsageDescription`) and is sandboxed
  with the audio-input entitlement. Use headphones with live input to avoid feedback.
- Launching the app once registers the embedded extension with the system, after which
  Live (and `auval`) can see it.

---

## 8. Ableton Live integration

- Component: type `aufx`, subtype `Smpv`, manufacturer `Crmc`.
  Name: `Craig McNamara: Sampling Reverb`.
- It needs to be registered: build it, copy `Sampling Reverb.app` to `/Applications`
  (or `~/Applications`) and launch it once. Then check:
  `auval -v aufx Smpv Crmc`.
- In Live: Settings → Plug-Ins → turn on *Use Audio Units v3* and rescan. This is separate
  from *Use Audio Units v2*, and without v3 Live never scans app-extension AUs. It appears
  under Plug-Ins → Audio Units → Craig McNamara.
- Every parameter is in the AU parameter tree, so Live can configure and automate it.
- State saves through `fullState` (the parameter values). The sample pool isn't saved,
  because it's ephemeral by design.

---

## 9. Real-time safety rules
- All buffers (input history ring, 24 slots × 2 s × stereo, delay lines) are allocated
  in `allocateRenderResources`, sized for the current sample rate.
- The render path has no `new`, `malloc`, locks, Objective-C messaging or Swift.
- Parameter changes reach the kernel through relaxed `std::atomic<float>` values, set
  from the parameter tree's `implementorValueObserver`.
- Visualization is published through a single-producer/single-consumer triple buffer.

---

## 10. Repository layout

```
project.yml                 XcodeGen spec (app + AUv3 extension)
App/                        Standalone SwiftUI host app
Extension/                  AUv3 extension
  AU/                       ObjC++ AUAudioUnit subclass + bridging header
  DSP/                      Header-only C++ DSP (kernel, reverb, pool, analyzer…)
  UI/                       SwiftUI controls, Metal scope renderer, shaders
  UI/Logic/                 Pure-Swift UI logic (mapping, plot space, particles, slot events)
Tests/DSP/                  C++ DSP tests (dependency-free harness, `make test-dsp`)
Tests/Swift/                Swift Testing suites for UI/Logic (`make test-swift`)
docs/design.md              This document
```

---

## 11. Testing

Tests are written first wherever it's practical.

- **C++ DSP** (`Tests/DSP`, about 60 cases). This is a tiny built-in harness, built with
  clang and with no dependencies. It covers:
  - the building blocks (delay lines, allpass energy, the SVF, the triple buffer under
    concurrency)
  - pitch-shifter accuracy, RT60 tracking, freeze and stability under extreme settings
  - engine behaviour: pitch and brightness analysis, capture, matching, Harmony, Focus,
    Retention and life extension, pool replacement, Freeze and Density
  - kernel contracts: bypass and dry are bit-exact, in-place processing, any block size,
    sample rates from 44.1 to 192 kHz, and random-parameter abuse
  - Shimmer and Sub regressions: no runaway (at most +6 dB) for every interval with stereo
    and left-only input, balanced wet output for left-only input, and the shifted pitch
    clearly present when on and absent when off
  - presets: Default matches the parameter defaults, every value is within range, presets
    are distinct, and every preset rings stably with real input
- **Swift** (`Tests/Swift`, Swift Testing). The UI logic that doesn't depend on AppKit or
  Metal lives in `Extension/UI/Logic` and is compiled straight into a hostless test bundle.
  Those tests cover parameter mapping, plot coordinates, the particle system (including
  the Metal vertex layout) and slot-change → animation events.
- **Integration**: `make validate` runs Apple's `auval`.

---

## 12. Roadmap / later ideas
- Save the sample pool with the Live set (optional, off by default).
- Pitch-match playback: retune a matched sample to the input pitch.
- Reverse and half-speed voice playback modes (NightSky-style "Texture" variants).
- A NightSky-style step sequencer that modulates Filter.
- MIDI input to trigger slots directly.
- A CLAP/VST3 wrapper around the same C++ DSP core.
