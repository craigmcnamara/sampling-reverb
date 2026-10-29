//! The whole kernel: routing, buffers, sample rates, pitch layers, tempo, quantize and sync.

mod common;

use common::*;
use sampling_reverb_dsp::kernel::{Kernel, SRHostTime};
use sampling_reverb_dsp::params::*;
use sampling_reverb_dsp::util::{lerpf, Rng};
use sampling_reverb_dsp::visual::{SRSlotVisual, SR_MAX_SLOTS, SR_SCOPE_POINTS};

const SR: f32 = 48000.0;

struct KernelRig {
    kernel: Kernel,
    sr: f32,
    host: SRHostTime,
    out_l: Buffer,
    out_r: Buffer,
    block: usize,
}

impl KernelRig {
    fn new() -> KernelRig {
        KernelRig::at(SR)
    }
    fn at(sr: f32) -> KernelRig {
        let kernel = Kernel::new();
        unsafe { kernel.prepare(sr as f64) };
        let rig =
            KernelRig { kernel, sr, host: SRHostTime::default(), out_l: Vec::new(), out_r: Vec::new(), block: 256 };
        rig.set(Param::Length, 0.8); // short captures keep the tests quick; the default is longer
        rig
    }
    fn set(&self, p: Param, v: f32) {
        self.kernel.set_parameter(p as u64, v);
    }
    fn run_stereo(&mut self, l: &[f32], r: &[f32]) {
        for (cl, cr) in l.chunks(self.block).zip(r.chunks(self.block)) {
            let (mut ol, mut or) = (vec![0.0; cl.len()], vec![0.0; cl.len()]);
            unsafe { self.kernel.process(&self.host, cl, cr, &mut ol, &mut or) };
            self.out_l.extend(ol);
            self.out_r.extend(or);
        }
    }
    fn run(&mut self, mono: &[f32]) {
        self.run_stereo(mono, mono);
    }
    fn wait(&mut self, seconds: f32) {
        self.run(&silence(seconds, self.sr));
    }
    fn triggers(&self) -> u64 {
        unsafe { self.kernel.engine().total_triggers() }
    }
    fn pool(&self) -> i32 {
        unsafe { self.kernel.engine().pool_count() }
    }
    /// Length of the longest captured slot, in seconds.
    fn longest_slot(&self) -> f32 {
        (0..SR_MAX_SLOTS).map(|s| unsafe { self.kernel.engine().slot_length(s) } as f32 / self.sr).fold(0.0, f32::max)
    }
}

fn tone(hz: f32, seconds: f32) -> Buffer {
    tone_burst(hz, seconds, 0.5, SR)
}

// ---- Parameters ----------------------------------------------------------------------------------

#[test]
fn defaults_match_the_parameter_table_and_specs_are_ordered() {
    let k = Kernel::new();
    for (i, spec) in PARAM_SPECS.iter().enumerate() {
        assert_eq!(spec.address as usize, i);
        assert_eq!(k.get_parameter(i as u64), spec.def);
    }
}

#[test]
fn parameter_roundtrip_and_bad_addresses_are_ignored() {
    let k = Kernel::new();
    k.set_parameter(Param::Mix as u64, 12.0);
    assert_eq!(k.get_parameter(Param::Mix as u64), 12.0);
    k.set_parameter(PARAM_COUNT as u64 + 5, 1.0);
    assert_eq!(k.get_parameter(PARAM_COUNT as u64 + 5), 0.0);
}

#[test]
fn length_goes_to_twenty_seconds() {
    let length = &PARAM_SPECS[Param::Length as usize];
    assert_eq!(length.max, 20.0);
    assert!(length.exponential); // short lengths stay easy to dial in
    assert!(length.def >= 2.0);
}

// ---- Routing -------------------------------------------------------------------------------------

#[test]
fn bypass_passes_input_through_exactly() {
    let mut rig = KernelRig::new();
    rig.kernel.set_bypass(true);
    let input = noise_burst(0.5, 0.5, SR, 1);
    rig.run(&input);
    assert_eq!(rig.out_l, input);
    assert_eq!(rig.out_r, input);
}

#[test]
fn mix_zero_is_dry() {
    let mut rig = KernelRig::new();
    rig.set(Param::Mix, 0.0);
    let input = noise_burst(0.5, 0.5, SR, 1);
    rig.run(&input);
    for (a, b) in rig.out_l.iter().zip(&input) {
        assert!((a - b).abs() < 1e-6);
    }
}

#[test]
fn wet_only_hears_just_samples_when_dry_feed_is_zero() {
    let mut rig = KernelRig::new();
    rig.set(Param::Mix, 100.0);
    rig.set(Param::DryFeed, 0.0);
    rig.run(&tone(220.0, 0.3)); // captured, never matched
    rig.wait(1.0);
    assert!(peak(&rig.out_l) < 1e-6 && peak(&rig.out_r) < 1e-6);
}

#[test]
fn matched_sound_plays_sample_into_reverb() {
    let mut rig = KernelRig::new();
    rig.set(Param::Mix, 100.0);
    rig.set(Param::DryFeed, 0.0);
    rig.run(&tone(220.0, 0.3));
    rig.wait(1.0);
    let before = rig.out_l.len();
    rig.run(&tone(220.0, 0.3));
    rig.wait(1.0);
    assert!(rms(&rig.out_l, before, rig.out_l.len()) > 1e-3);
    assert_eq!(rig.triggers(), 1);
}

#[test]
fn dry_feed_sends_live_input_to_the_reverb() {
    let mut rig = KernelRig::new();
    rig.set(Param::Mix, 100.0);
    rig.set(Param::DryFeed, 100.0);
    let mut impulse = silence(1.0, SR);
    impulse[0] = 1.0;
    rig.run(&impulse);
    assert!(rms(&rig.out_l, frames(0.2, SR), frames(0.5, SR)) > 1e-4);
}

#[test]
fn maps_parameters_onto_the_engine() {
    let mut rig = KernelRig::new();
    rig.set(Param::Retention, 0.0); // 3 s lifetime
    rig.run(&tone(220.0, 0.3));
    rig.wait(1.0);
    assert_eq!(rig.pool(), 1);
    rig.wait(3.5);
    assert_eq!(rig.pool(), 0);
}

// ---- Buffers and rates ---------------------------------------------------------------------------

#[test]
fn in_place_matches_out_of_place_at_any_block_size() {
    let mut input = strum(1.0, 0.5, SR);
    append(&mut input, &tone(440.0, 0.3));
    append(&mut input, &silence(1.0, SR));
    let mut reference = KernelRig::new();
    reference.run(&input);

    for block in [1usize, 7, 37, 255, 256, 257, 1000, 4096] {
        let rig = KernelRig::new();
        let (mut l, mut r) = (input.clone(), input.clone());
        let mut i = 0;
        while i < l.len() {
            let n = block.min(l.len() - i);
            // In place, as the AU does: the outputs are the input buffers.
            unsafe {
                let (pl, pr) = (l.as_mut_ptr().add(i), r.as_mut_ptr().add(i));
                rig.kernel.process_raw(&rig.host, pl, pr, pl, pr, n)
            };
            i += n;
        }
        assert_eq!(l, reference.out_l, "block {block}");
        assert_eq!(r, reference.out_r, "block {block}");
    }
}

#[test]
fn mono_in_place_input_is_read_before_it_is_overwritten() {
    let input = strum(1.0, 0.5, SR);
    let mut reference = KernelRig::new();
    reference.run(&input);
    let rig = KernelRig::new();
    let mut l = input.clone();
    let mut r = vec![0.0; l.len()];
    // inL == inR == outL (mono, in place), outR separate.
    unsafe { rig.kernel.process_raw(&rig.host, l.as_ptr(), l.as_ptr(), l.as_mut_ptr(), r.as_mut_ptr(), l.len()) };
    assert_eq!(l, reference.out_l);
    assert_eq!(r, reference.out_r);
}

#[test]
fn handles_odd_and_large_block_sizes() {
    for block in [1, 37, 255, 257, 4096] {
        let mut rig = KernelRig::new();
        rig.block = block;
        let mut input = tone(220.0, 0.4);
        append(&mut input, &silence(1.0, SR));
        rig.run(&input);
        assert!(all_finite(&rig.out_l));
        assert_eq!(rig.pool(), 1, "block {block}");
    }
}

#[test]
fn supports_common_sample_rates() {
    for rate in [44100.0f32, 48000.0, 88200.0, 96000.0, 176400.0, 192000.0] {
        let mut rig = KernelRig::at(rate);
        let mut input = tone_burst(220.0, 0.3, 0.5, rate);
        append(&mut input, &silence(1.0, rate));
        rig.run(&input);
        assert!(all_finite(&rig.out_l), "{rate}");
        assert_eq!(rig.pool(), 1, "{rate}");
        assert_near!(unsafe { rig.kernel.engine().last_features().midi }, 57.0, 0.35, "{rate}");
    }
}

#[test]
fn random_parameter_abuse_stays_finite_and_bounded() {
    let mut rig = KernelRig::new();
    let mut rng = Rng::default();
    let mut input = Buffer::new();
    for i in 0..40u32 {
        append(
            &mut input,
            &if i % 2 == 1 {
                noise_burst(0.25, 1.0, SR, i + 1)
            } else {
                tone_burst(110.0 * (1 + i % 5) as f32, 0.25, 1.0, SR)
            },
        );
    }
    for chunk in input.chunks(4096) {
        for spec in &PARAM_SPECS {
            rig.set(spec.address, lerpf(spec.min, spec.max, rng.uniform()));
        }
        rig.set(Param::Freeze, 0.0);
        rig.run(chunk);
    }
    assert!(all_finite(&rig.out_l) && all_finite(&rig.out_r));
    assert!(peak(&rig.out_l) < 8.0);
}

#[test]
fn survives_out_of_range_parameters_and_wild_host_times() {
    let mut rig = KernelRig::new();
    let mut rng = Rng::default();
    let mut input = Buffer::new();
    for i in 0..20u32 {
        append(
            &mut input,
            &if i % 3 == 0 { noise_burst(0.2, 0.9, SR, i) } else { tone_burst(100.0 + 50.0 * i as f32, 0.3, 0.9, SR) },
        );
    }
    for chunk in input.chunks(4800) {
        for spec in &PARAM_SPECS {
            // Anything in range, and now and then far outside it.
            let v = if rng.uniform() < 0.05 { rng.bipolar() * 1e6 } else { lerpf(spec.min, spec.max, rng.uniform()) };
            rig.set(spec.address, v);
        }
        rig.host = SRHostTime {
            has_tempo: rng.uniform() < 0.5,
            tempo: (rng.uniform() * 300.0) as f64,
            has_position: true,
            beat: (rng.bipolar() * 1000.0) as f64,
            downbeat: 0.0,
            bar_beats: (rng.uniform() * 12.0) as f64,
            has_transport: rng.uniform() < 0.5,
            playing: rng.uniform() < 0.5,
        };
        rig.block = 1 + (rng.uniform() * 2000.0) as usize;
        rig.run(chunk);
    }
    assert!(all_finite(&rig.out_l) && all_finite(&rig.out_r));
}

#[test]
fn publishes_visual_state() {
    let mut rig = KernelRig::new();
    rig.run(&tone(220.0, 0.3));
    rig.wait(1.0);
    let mut slots = [SRSlotVisual::default(); SR_MAX_SLOTS];
    let (mut l, mut r) = ([0f32; SR_SCOPE_POINTS], [0f32; SR_SCOPE_POINTS]);
    let summary = unsafe { rig.kernel.read_visual(&mut slots, &mut l, &mut r) };
    assert_eq!(summary.pool_count, 1);
    assert!(!summary.frozen);
    assert_eq!(slots.iter().filter(|s| s.active != 0).count(), 1);
    assert!(peak(&l) > 0.0); // reverb tail of the dry feed

    rig.set(Param::Freeze, 1.0);
    rig.wait(0.1);
    let summary = unsafe { rig.kernel.read_visual(&mut slots, &mut [], &mut []) };
    assert!(summary.frozen);
}

// ---- Shimmer and Sub -----------------------------------------------------------------------------

#[test]
fn turning_up_shimmer_keeps_the_reverb_audible() {
    for shimmer in [25.0, 50.0, 100.0] {
        let mut rig = KernelRig::new();
        rig.set(Param::Mix, 100.0); // wet only, so "goes dry" shows up as silence
        let mut phrase = Buffer::new();
        for i in 0..4 {
            append(&mut phrase, &tone(if i % 2 == 1 { 330.0 } else { 220.0 }, 0.25));
            append(&mut phrase, &silence(0.25, SR));
        }
        rig.run(&phrase); // 2 s with shimmer off
        rig.set(Param::Shimmer, shimmer);
        for _ in 0..5 {
            rig.run(&phrase); // 10 s with shimmer on
        }
        assert!(all_finite(&rig.out_l) && all_finite(&rig.out_r));
        let mut t = 2.0;
        while t < 12.0 {
            let level = rms(&rig.out_l, frames(t, SR), frames(t + 0.5, SR));
            assert!(level > 1e-3, "shimmer {shimmer}%: wet level {level} at {t:.1} s");
            t += 0.5;
        }
        assert!(peak(&rig.out_l) < 4.0);
    }
}

/// Plays a phrase of notes (wet only) and returns per-channel wet RMS over the last 4 s.
fn late_wet_levels(shimmer: f32, interval: usize, left_only: bool, sub: f32) -> (f32, f32) {
    let mut rig = KernelRig::new();
    rig.set(Param::Mix, 100.0);
    rig.set(Param::Sub, sub);
    rig.set(Param::Shimmer, shimmer);
    rig.set(Param::Interval, interval as f32);
    let mut l = Buffer::new();
    for i in 0..20 {
        append(&mut l, &tone(if i % 2 == 1 { 330.0 } else { 220.0 }, 0.25));
        append(&mut l, &silence(0.25, SR));
    }
    let r = if left_only { vec![0.0; l.len()] } else { l.clone() };
    rig.run_stereo(&l, &r);
    let from = l.len() - frames(4.0, SR);
    (rms(&rig.out_l, from, l.len()), rms(&rig.out_r, from, l.len()))
}

#[test]
fn shimmer_never_runs_away() {
    for left_only in [false, true] {
        let base = late_wet_levels(0.0, 4, left_only, 0.0);
        for (interval, name) in INTERVAL_NAMES.iter().enumerate() {
            let shim = late_wet_levels(100.0, interval, left_only, 0.0);
            let (gain_l, gain_r) = (db(shim.0) - db(base.0), db(shim.1) - db(base.1));
            assert!(
                gain_l <= 6.0 && gain_r <= 6.0,
                "left-only {left_only}, interval {name:?}: shimmer raised wet by L {gain_l:+.1} dB, R {gain_r:+.1} dB"
            );
        }
    }
}

#[test]
fn sub_never_runs_away() {
    for left_only in [false, true] {
        let base = late_wet_levels(0.0, 4, left_only, 0.0);
        for shimmer in [0.0, 100.0] {
            let sub = late_wet_levels(shimmer, 4, left_only, 100.0);
            let (gain_l, gain_r) = (db(sub.0) - db(base.0), db(sub.1) - db(base.1));
            assert!(
                gain_l <= 6.0 && gain_r <= 6.0,
                "left-only {left_only}, shimmer {shimmer}%: L {gain_l:+.1} dB, R {gain_r:+.1} dB"
            );
        }
    }
}

#[test]
fn left_only_input_gives_balanced_wet() {
    for shimmer in [0.0, 100.0] {
        let (l, r) = late_wet_levels(shimmer, 4, true, 0.0);
        let balance = db(l) - db(r);
        assert!(balance.abs() <= 6.0, "shimmer {shimmer}%: L-R wet balance {balance:+.1} dB");
    }
}

/// Hann-windowed DFT energy summed over [lo, hi] Hz (1 Hz steps) from `from` on. The band is wide
/// because a grain pitch shifter spreads its output into sidebands around the target pitch.
fn dft_band_energy(x: &[f32], from: usize, lo: f32, hi: f32) -> f64 {
    let n = x.len() - from;
    let pi = std::f64::consts::PI;
    let mut total = 0.0;
    let mut f = lo;
    while f <= hi {
        let w = 2.0 * pi * f as f64 / SR as f64;
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for i in 0..n {
            let hann = 0.5 - 0.5 * (2.0 * pi * i as f64 / n as f64).cos();
            let v = x[from + i] as f64 * hann;
            re += v * (w * i as f64).cos();
            im -= v * (w * i as f64).sin();
        }
        total += re * re + im * im;
        f += 1.0;
    }
    total
}

/// Level of the shifted pitch relative to the 220 Hz fundamental, in dB.
fn shifted_vs_fundamental(shimmer: f32, interval: usize, shifted_hz: f32, sub: f32) -> f32 {
    let mut rig = KernelRig::new();
    rig.set(Param::Mix, 100.0);
    rig.set(Param::Decay, 8.0);
    rig.set(Param::Mod, 0.0);
    rig.set(Param::Sub, sub);
    rig.set(Param::Shimmer, shimmer);
    rig.set(Param::Interval, interval as f32);
    rig.run(&sine(220.0, 4.0, 0.3, SR));
    let from = frames(3.0, SR);
    let shifted = dft_band_energy(&rig.out_l, from, shifted_hz - 40.0, shifted_hz + 40.0);
    let fundamental = dft_band_energy(&rig.out_l, from, 205.0, 235.0);
    10.0 * (shifted / fundamental).log10() as f32
}

#[test]
fn shimmer_adds_energy_at_the_shifted_pitch() {
    let (octave_up, octave_down) = (4, 0); // indexes into INTERVAL_SEMITONES
    let up = shifted_vs_fundamental(100.0, octave_up, 440.0, 0.0);
    let down = shifted_vs_fundamental(100.0, octave_down, 110.0, 0.0);
    let off = shifted_vs_fundamental(0.0, octave_up, 440.0, 0.0);
    assert!(up > -12.0 && down > -12.0 && off < -25.0, "+12 {up:.1} dB, -12 {down:.1} dB, off {off:.1} dB");

    // Sub: an octave down, independent of Shimmer and Interval, and no upward shimmer.
    let sub = shifted_vs_fundamental(0.0, octave_up, 110.0, 100.0);
    let sub_off = shifted_vs_fundamental(0.0, octave_up, 110.0, 0.0);
    let sub_up = shifted_vs_fundamental(0.0, octave_up, 440.0, 100.0);
    assert!(sub > -12.0 && sub_off < -25.0 && sub_up < -25.0, "sub {sub:.1}, off {sub_off:.1}, 440 Hz {sub_up:.1} dB");
}

/// Wet output of a few strummed guitar chords followed by the tail.
fn wet_guitar(params: &[(Param, f32)]) -> Buffer {
    let mut rig = KernelRig::new();
    rig.set(Param::Mix, 100.0);
    rig.set(Param::Decay, 8.0);
    for (p, v) in params {
        rig.set(*p, *v);
    }
    let mut input = Buffer::new();
    for _ in 0..4 {
        append(&mut input, &strum(2.0, 0.5, SR));
    }
    append(&mut input, &silence(6.0, SR));
    rig.run(&input);
    rig.out_l
}

#[test]
fn pitch_layers_keep_the_reverbs_body() {
    // Shimmer and Sub should add layers, not hollow out the guitar range (125 Hz – 1 kHz).
    let from = frames(4.0, SR);
    let body = |params: &[(Param, f32)]| band_energy_db(&wet_guitar(params), from, 125.0, 1000.0, SR);
    let plain = body(&[]);
    let shimmer = body(&[(Param::Shimmer, 50.0)]);
    let sub = body(&[(Param::Sub, 50.0)]);
    assert!(shimmer - plain > -3.0, "shimmer {:+.1} dB", shimmer - plain);
    assert!(sub - plain > -3.0, "sub {:+.1} dB", sub - plain);
}

#[test]
fn sub_adds_the_undertone_series() {
    // A held 330 Hz note with Sub up gains energy at f/2, f/3 and f/4.
    let levels = |sub: f32| -> [f32; 3] {
        let mut rig = KernelRig::new();
        rig.set(Param::Mix, 100.0);
        rig.set(Param::Decay, 8.0);
        rig.set(Param::Mod, 0.0);
        rig.set(Param::Sub, sub);
        rig.run(&sine(330.0, 4.0, 0.3, SR));
        let (from, to) = (frames(2.0, SR), rig.out_l.len());
        let f0 = tone_level(&rig.out_l, from, to, 330.0, SR);
        [165.0, 110.0, 82.5].map(|hz: f32| {
            // Grain shifters spread energy into sidebands, so take the strongest bin within ±12 Hz.
            let best = (-12..=12).map(|d| tone_level(&rig.out_l, from, to, hz + d as f32, SR)).fold(0.0, f32::max);
            db(best) - db(f0)
        })
    };
    let (on, off) = (levels(100.0), levels(0.0));
    for k in 0..3 {
        assert!(on[k] > -24.0 && on[k] - off[k] > 15.0, "undertone {k}: on {:.1} dB, off {:.1} dB", on[k], off[k]);
    }
}

#[test]
fn attack_parameter_reaches_the_voices() {
    // Wet-only, samples only: compare how loud the replayed sample is in its first 30 ms.
    let onset = |attack_ms: f32| {
        let mut rig = KernelRig::new();
        rig.set(Param::Mix, 100.0);
        rig.set(Param::DryFeed, 0.0);
        rig.set(Param::Decay, 0.3);
        rig.set(Param::Attack, attack_ms);
        rig.run(&tone(220.0, 0.4));
        rig.wait(1.0);
        let start = rig.out_l.len();
        rig.run(&tone(220.0, 0.4));
        let voice = (start..rig.out_l.len()).find(|&i| rig.out_l[i].abs() >= 1e-7).unwrap_or(rig.out_l.len());
        rms(&rig.out_l, voice, voice + frames(0.03, SR))
    };
    let (fast, slow) = (onset(1.0), onset(1000.0));
    assert!(fast > 1e-3);
    assert!(slow < 0.25 * fast);
}

// ---- Tempo and quantize --------------------------------------------------------------------------

/// Captures a note, then replays it `offset` frames after second 3, telling the kernel the host time
/// before each 16-frame block. Returns (trigger frame, onset frame), to within one block.
fn quantized_replay(rig: &mut KernelRig, offset: usize, host: impl Fn(u64) -> SRHostTime) -> (u64, u64) {
    rig.block = 16;
    let clock = std::cell::Cell::new(0u64);
    let feed = |rig: &mut KernelRig, input: &[f32]| -> Option<u64> {
        let before = rig.triggers();
        let mut fired = None;
        for chunk in input.chunks(16) {
            rig.host = host(clock.get());
            rig.run(chunk);
            if fired.is_none() && rig.triggers() != before {
                fired = Some(clock.get());
            }
            clock.set(clock.get() + chunk.len() as u64);
        }
        fired
    };
    feed(rig, &tone(440.0, 0.3));
    let gap = frames(3.0, SR) - clock.get() as usize + offset;
    feed(rig, &vec![0.0; gap]);
    let onset = clock.get();
    let mut replay = tone(440.0, 0.3);
    append(&mut replay, &silence(2.0, SR)); // long enough to reach a whole-bar grid point
    (feed(rig, &replay).expect("triggered"), onset)
}

fn playing_host(
    tempo: f64,
    start_beat: f64,
    bar_beats: f64,
    downbeat: impl Fn(f64) -> f64,
) -> impl Fn(u64) -> SRHostTime {
    let bpf = tempo / (60.0 * SR as f64);
    move |frame| {
        let beat = start_beat + frame as f64 * bpf;
        SRHostTime {
            has_tempo: true,
            tempo,
            has_position: true,
            beat,
            downbeat: downbeat(beat),
            bar_beats,
            has_transport: true,
            playing: true,
        }
    }
}

#[test]
fn quantize_follows_the_host_transport() {
    let mut rig = KernelRig::new();
    rig.set(Param::Length, 0.5);
    rig.set(Param::Quantize, 1.0);
    rig.set(Param::Grid, 5.0); // 1/4
    rig.set(Param::Tempo, 77.0); // ignored: the host has a tempo
                                 // The host plays at 100 BPM from beat 10.4, so quarter notes fall where beat is a whole number.
    let bpf = 100.0 / (60.0 * SR as f64);
    let (fired, onset) = quantized_replay(&mut rig, 5000, playing_host(100.0, 10.4, 4.0, |_| 8.0));
    let beat = 10.4 + fired as f64 * bpf;
    assert!((beat - beat.round()).abs() < 16.0 * bpf, "fired at beat {beat}");
    assert!(fired > onset + 2048);
    assert!(rig.kernel.clock().from_host);
    assert_eq!(rig.kernel.clock().tempo, 100.0);
}

#[test]
fn quantize_runs_free_at_the_tempo_parameter_without_a_host() {
    let mut rig = KernelRig::new();
    rig.set(Param::Length, 0.5);
    rig.set(Param::Quantize, 1.0);
    rig.set(Param::Grid, 3.0); // 1/8
    rig.set(Param::Tempo, 90.0); // 1/8 = 16000 frames
    let (fired, onset) = quantized_replay(&mut rig, 7000, |_| SRHostTime::default());
    assert!(fired % 16000 < 16 || fired % 16000 > 16000 - 16, "fired at {fired}");
    assert!(fired > onset + 2048);
    assert!(!rig.kernel.clock().from_host);
    assert_eq!(rig.kernel.clock().tempo, 90.0);
}

#[test]
fn stopped_transport_keeps_a_running_grid_at_host_tempo() {
    // Stopped, the host's beat stays put; the grid must keep running instead of stalling there.
    let mut rig = KernelRig::new();
    rig.set(Param::Length, 0.5);
    rig.set(Param::Quantize, 1.0);
    rig.set(Param::Grid, 3.0);
    let stopped = |_| SRHostTime {
        has_tempo: true,
        tempo: 120.0,
        has_position: true,
        beat: 33.3,
        has_transport: true,
        playing: false,
        ..Default::default()
    };
    let (fired, onset) = quantized_replay(&mut rig, 2000, stopped);
    assert!(fired > onset + 2048);
    assert!(fired <= onset + 2048 + 12000 + 16);
}

#[test]
fn grid_bar_uses_the_host_time_signature() {
    // 3/4 at 120 BPM: a bar is 3 beats = 72000 frames, counted from the downbeat at beat 0.
    let mut rig = KernelRig::new();
    rig.set(Param::Length, 0.5);
    rig.set(Param::Quantize, 1.0);
    rig.set(Param::Grid, 7.0);
    let (fired, onset) = quantized_replay(&mut rig, 1000, playing_host(120.0, 0.0, 3.0, |b| (b / 3.0).floor() * 3.0));
    assert!(fired % 72000 < 16 || fired % 72000 > 72000 - 16, "fired at {fired}");
    assert!(fired > onset + 2048);
}

// ---- Synced Pre-delay and Length -----------------------------------------------------------------

/// Seconds from an impulse to the first wet output, with only the live input feeding the reverb.
fn wet_arrival(params: &[(Param, f32)]) -> f32 {
    let mut rig = KernelRig::new();
    rig.set(Param::Mix, 100.0);
    rig.set(Param::DryFeed, 100.0);
    rig.set(Param::SampleLevel, 0.0);
    for (p, v) in params {
        rig.set(*p, *v);
    }
    let mut input = silence(0.1, SR); // let the pre-delay settle on its target
    let at = input.len();
    input.push(1.0);
    append(&mut input, &silence(4.0, SR));
    rig.run(&input);
    let i =
        (at..rig.out_l.len()).find(|&i| rig.out_l[i].abs() > 1e-4 || rig.out_r[i].abs() > 1e-4).expect("wet output");
    (i - at) as f32 / SR
}

#[test]
fn synced_pre_delay_follows_the_tempo() {
    let base = wet_arrival(&[(Param::PreDelay, 0.0)]);
    // 1/8 at 120 BPM = 250 ms; 1/4D at 60 BPM = 1.5 s, past the free knob's 500 ms; 1/8T at 100 = 200 ms.
    let eighth = wet_arrival(&[(Param::Sync, 1.0), (Param::PreDelayDiv, 7.0), (Param::Tempo, 120.0)]);
    let dotted_quarter = wet_arrival(&[(Param::Sync, 1.0), (Param::PreDelayDiv, 11.0), (Param::Tempo, 60.0)]);
    let triplet = wet_arrival(&[(Param::Sync, 1.0), (Param::PreDelayDiv, 6.0), (Param::Tempo, 100.0)]);
    assert_near!(eighth - base, 0.25, 0.002);
    assert_near!(dotted_quarter - base, 1.5, 0.002);
    assert_near!(triplet - base, 0.2, 0.002);
}

#[test]
fn sync_off_keeps_the_free_pre_delay() {
    let base = wet_arrival(&[(Param::PreDelay, 0.0)]);
    let free = wet_arrival(&[(Param::PreDelay, 100.0), (Param::PreDelayDiv, 12.0)]);
    assert_near!(free - base, 0.1, 0.002);
}

#[test]
fn synced_length_captures_whole_beats_and_bars() {
    let captured = |div: f32, tempo: f32| {
        let mut rig = KernelRig::new();
        rig.set(Param::Sync, 1.0);
        rig.set(Param::LengthDiv, div);
        rig.set(Param::Tempo, tempo);
        rig.wait(0.1); // room for the whole pre-roll
        rig.run(&tone(330.0, 0.3));
        rig.wait(21.0);
        rig.longest_slot()
    };
    let preroll = 0.01;
    assert_near!(captured(1.0, 120.0), 0.5 + preroll, 0.001); // 1/4 at 120
    assert_near!(captured(3.0, 90.0), 8.0 / 3.0 + preroll, 0.001); // 1 bar of 4/4 at 90
    assert_near!(captured(5.0, 160.0), 6.0 + preroll, 0.001); // 4 bars at 160
    assert_near!(captured(6.0, 60.0), 20.0 + preroll, 0.001); // 8 bars at 60 is 32 s: capped at 20 s
}

#[test]
fn synced_bar_lengths_follow_the_host_time_signature() {
    let mut rig = KernelRig::new();
    rig.set(Param::Sync, 1.0);
    rig.set(Param::LengthDiv, 4.0); // 2 bars
                                    // 3/4 at 120 BPM: 2 bars = 6 beats = 3 s.
    rig.host = SRHostTime { has_tempo: true, tempo: 120.0, has_position: true, bar_beats: 3.0, ..Default::default() };
    rig.wait(0.1);
    rig.run(&tone(330.0, 0.3));
    rig.wait(4.0);
    assert_near!(rig.longest_slot(), 3.01, 0.001);
}

#[test]
fn clock_reports_the_tempo_and_its_source() {
    let mut rig = KernelRig::new();
    rig.set(Param::Tempo, 97.5);
    rig.wait(0.1);
    assert_eq!(rig.kernel.clock().tempo, 97.5);
    assert!(!rig.kernel.clock().from_host);
    rig.host = SRHostTime { has_tempo: true, tempo: 121.25, ..Default::default() };
    rig.wait(0.1);
    assert_eq!(rig.kernel.clock().tempo, 121.25);
    assert!(rig.kernel.clock().from_host);
}
