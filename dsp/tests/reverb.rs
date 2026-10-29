//! The FDN reverb and its pitch shifter.

mod common;

use common::*;
use sampling_reverb_dsp::reverb::{FdnReverb, PitchShifter, ReverbParams};

const SR: f32 = 48000.0;

struct ReverbRig {
    reverb: FdnReverb,
    out_l: Buffer,
    out_r: Buffer,
}

impl ReverbRig {
    fn new(p: ReverbParams) -> ReverbRig {
        let mut reverb = FdnReverb::default();
        reverb.prepare(SR);
        reverb.set_params(&p);
        ReverbRig { reverb, out_l: Vec::new(), out_r: Vec::new() }
    }
    fn run(&mut self, input: &[f32]) {
        for chunk in input.chunks(256) {
            let (mut l, mut r) = (vec![0.0; chunk.len()], vec![0.0; chunk.len()]);
            self.reverb.process(chunk, chunk, &mut l, &mut r);
            self.out_l.extend(l);
            self.out_r.extend(r);
        }
    }
}

fn defaults() -> ReverbParams {
    ReverbParams { decay_sec: 2.0, size: 0.5, tone: 1.0, texture: 0.5, modulation: 0.0, ..Default::default() }
}

fn shifted(semitones: f32, hz: f32) -> Buffer {
    let mut ps = PitchShifter::default();
    ps.prepare(SR);
    ps.set_semitones(semitones);
    sine(hz, 1.0, 0.5, SR).into_iter().map(|x| ps.process(x)).collect()
}

#[test]
fn pitch_shifter_shifts_up_an_octave() {
    let y = shifted(12.0, 220.0);
    assert_near!(zero_crossing_hz(&y, 4800, y.len(), SR), 440.0, 15.0);
}

#[test]
fn pitch_shifter_shifts_down_a_fifth() {
    let y = shifted(-5.0, 440.0);
    assert_near!(zero_crossing_hz(&y, 4800, y.len(), SR), 440.0 * 2f32.powf(-5.0 / 12.0), 12.0);
}

#[test]
fn reverb_impulse_produces_decorrelated_stereo_tail() {
    let mut rig = ReverbRig::new(defaults());
    let mut impulse = silence(1.0, SR);
    impulse[0] = 1.0;
    rig.run(&impulse);
    let (from, to) = (frames(0.2, SR), frames(0.4, SR));
    assert!(rms(&rig.out_l, from, to) > 1e-3);
    assert!(rms(&rig.out_r, from, to) > 1e-3);
    let diff: f64 = (from..to).map(|i| (rig.out_l[i] - rig.out_r[i]).abs() as f64).sum();
    assert!(diff > 1.0);
}

#[test]
fn reverb_decay_time_tracks_rt60() {
    for rt60 in [1.0f32, 4.0] {
        let mut rig = ReverbRig::new(ReverbParams { decay_sec: rt60, ..defaults() });
        let mut noise = noise_burst(0.05, 0.5, SR, 1);
        noise.resize(frames(3.0, SR), 0.0);
        rig.run(&noise);
        // Expected level drop between two windows 0.5 s apart: 60 dB * 0.5 / RT60.
        let a = rms(&rig.out_l, frames(0.3, SR), frames(0.4, SR));
        let b = rms(&rig.out_l, frames(0.8, SR), frames(0.9, SR));
        let expected = -60.0 * 0.5 / rt60;
        assert_near!(db(b) - db(a), expected, expected.abs() * 0.35 + 1.0);
    }
}

#[test]
fn reverb_freeze_holds_the_tail_and_ignores_input() {
    let mut p = ReverbParams { decay_sec: 30.0, ..defaults() };
    let mut rig = ReverbRig::new(p);
    rig.run(&noise_burst(0.5, 0.5, SR, 1));
    p.freeze = true;
    rig.reverb.set_params(&p);
    rig.run(&noise_burst(3.0, 0.5, SR, 7)); // input during freeze must be ignored
    let base = frames(0.5, SR);
    let early = rms(&rig.out_l, base + frames(0.5, SR), base + frames(1.0, SR));
    let late = rms(&rig.out_l, base + frames(2.5, SR), base + frames(3.0, SR));
    assert!(early > 1e-3);
    assert!((db(late) - db(early)).abs() < 3.0);
}

#[test]
fn reverb_freeze_from_silence_stays_silent() {
    let mut rig = ReverbRig::new(ReverbParams { freeze: true, ..defaults() });
    rig.run(&noise_burst(0.5, 0.5, SR, 1));
    assert_eq!(peak(&rig.out_l), 0.0);
}

#[test]
fn reverb_extreme_settings_stay_bounded() {
    let mut rig = ReverbRig::new(ReverbParams {
        decay_sec: 30.0,
        size: 1.0,
        texture: 1.0,
        modulation: 1.0,
        shimmer: 1.0,
        shimmer_semitones: 19.0,
        ..defaults()
    });
    rig.run(&noise_burst(10.0, 1.0, SR, 3));
    assert!(all_finite(&rig.out_l) && all_finite(&rig.out_r));
    assert!(peak(&rig.out_l) < 8.0);
}

#[test]
fn reverb_freeze_holds_steady_with_shimmer_and_sub() {
    // At a loop gain of exactly 1, pitch shifting in the loop compounds forever: Shimmer used to climb
    // ~40 dB a minute into the limiter and Sub drained the pad. Frozen, the loop holds what it has.
    for (shimmer, sub) in [(0.6, 0.0), (0.0, 0.6), (1.0, 1.0)] {
        let mut p = ReverbParams { decay_sec: 4.0, tone: 0.6, shimmer, sub, modulation: 0.5, ..defaults() };
        let mut rig = ReverbRig::new(p);
        rig.run(&noise_burst(2.0, 0.5, SR, 1));
        p.freeze = true;
        rig.reverb.set_params(&p);
        rig.run(&silence(60.0, SR));
        let level = |t: f32| db(rms(&rig.out_l, frames(2.0 + t, SR), frames(2.5 + t, SR)));
        assert!(level(1.0) > -70.0, "shimmer {shimmer} sub {sub}: frozen pad is silent");
        for t in [5.0, 20.0, 59.0] {
            let drift = level(t) - level(1.0);
            assert!(drift.abs() < 1.0, "shimmer {shimmer} sub {sub}: {drift:+.1} dB after {t} s frozen");
        }
    }
}
