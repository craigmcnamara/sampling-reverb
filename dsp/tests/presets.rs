//! Factory presets: names, ranges, stability, and Night Sky's guitar voicing.

mod common;

use common::*;
use sampling_reverb_dsp::kernel::{Kernel, SRHostTime};
use sampling_reverb_dsp::params::*;

const SR: f32 = 48000.0;

fn render(values: &[f32; PARAM_COUNT], overrides: &[(Param, f32)], input: &[f32]) -> (Buffer, Buffer) {
    let k = Kernel::new();
    for (p, v) in values.iter().enumerate() {
        k.set_parameter(p as u64, *v);
    }
    for (p, v) in overrides {
        k.set_parameter(*p as u64, *v);
    }
    unsafe { k.prepare(SR as f64) };
    let (mut l, mut r) = (vec![0.0; input.len()], vec![0.0; input.len()]);
    for ((ci, cl), cr) in input.chunks(512).zip(l.chunks_mut(512)).zip(r.chunks_mut(512)) {
        unsafe { k.process(&SRHostTime::default(), ci, ci, cl, cr) };
    }
    (l, r)
}

#[test]
fn there_are_seven_with_unique_short_names() {
    assert_eq!(PRESET_COUNT, 7);
    for (i, a) in PRESETS.iter().enumerate() {
        assert!(!a.name.is_empty() && a.name.to_bytes().len() <= 16); // must fit a toolbar button
        for b in &PRESETS[i + 1..] {
            assert_ne!(a.name, b.name);
        }
    }
}

#[test]
fn values_are_within_parameter_ranges() {
    for preset in &PRESETS {
        for spec in &PARAM_SPECS {
            let v = preset.values[spec.address as usize];
            assert!(v >= spec.min && v <= spec.max, "{:?}: {:?} = {v}", preset.name, spec.identifier);
            if matches!(spec.unit, Unit::Indexed | Unit::Boolean) || spec.address == Param::Density {
                assert_eq!(v, v.round(), "{:?}: {:?}", preset.name, spec.identifier);
            }
        }
    }
}

#[test]
fn are_distinct_from_each_other() {
    for (i, a) in PRESETS.iter().enumerate() {
        for b in &PRESETS[i + 1..] {
            let differing = a.values.iter().zip(&b.values).filter(|(x, y)| x != y).count();
            assert!(differing >= 5, "{:?} vs {:?}", a.name, b.name);
        }
    }
}

#[test]
fn first_is_default_and_matches_parameter_defaults() {
    assert_eq!(PRESETS[0].name.to_str().unwrap(), "Default");
    for spec in &PARAM_SPECS {
        assert_eq!(PRESETS[0].values[spec.address as usize], spec.def, "{:?}", spec.identifier);
    }
}

#[test]
fn leave_the_clock_and_freeze_at_their_defaults() {
    for preset in &PRESETS {
        assert_eq!(preset.values[Param::Freeze as usize], 0.0);
        for spec in &PARAM_SPECS {
            if preset_leaves_alone(spec.address as u64) {
                assert_eq!(preset.values[spec.address as usize], spec.def);
            }
        }
    }
    for p in [Param::Freeze, Param::Tempo, Param::Quantize, Param::Grid, Param::Sync] {
        assert!(preset_leaves_alone(p as u64));
    }
    assert!(!preset_leaves_alone(Param::PreDelayDiv as u64) && !preset_leaves_alone(Param::LengthDiv as u64));
}

#[test]
fn some_preset_uses_sub() {
    assert!(PRESETS.iter().any(|p| p.values[Param::Sub as usize] > 0.0));
}

#[test]
fn all_sound_stable_and_audible() {
    let mut phrase = Buffer::new();
    for i in 0..6u32 {
        append(
            &mut phrase,
            &if i % 3 == 2 {
                noise_burst(0.3, 0.5, SR, i + 1)
            } else {
                tone_burst(if i % 2 == 1 { 330.0 } else { 220.0 }, 0.3, 0.5, SR)
            },
        );
        append(&mut phrase, &silence(0.5, SR));
    }
    // Play, wait out the longest Length so every capture lands, then play again: presets with Dry
    // Feed at 0 only make sound once samples are replayed.
    let longest = PRESETS.iter().map(|p| p.values[Param::Length as usize]).fold(0.0, f32::max);
    let mut input = phrase.clone();
    append(&mut input, &silence(longest + 1.0, SR));
    append(&mut input, &phrase);
    append(&mut input, &silence(2.0, SR));

    for preset in &PRESETS {
        let (l, r) = render(&preset.values, &[], &input);
        let tail = rms(&l, input.len() - frames(2.0, SR), input.len() - frames(1.5, SR));
        assert!(all_finite(&l) && all_finite(&r), "{:?}", preset.name);
        assert!(peak(&l).max(peak(&r)) < 4.0, "{:?}", preset.name);
        assert!(tail > 1e-5, "{:?}: a reverb tail is still ringing after the last note ({tail})", preset.name);
    }
}

#[test]
fn night_sky_is_voiced_for_guitar() {
    // Against the same preset with Shimmer and Sub off, on strummed guitar: the pitch layers must add
    // warmth underneath, must not add chirpy brightness up top, and must keep the body.
    let night_sky = PRESETS.iter().find(|p| p.name.to_str() == Ok("Night Sky")).expect("Night Sky");
    let mut input = Buffer::new();
    for _ in 0..4 {
        append(&mut input, &strum(2.0, 0.5, SR));
    }
    append(&mut input, &silence(8.0, SR));
    let (with, _) = render(&night_sky.values, &[(Param::Mix, 100.0)], &input);
    let (without, _) =
        render(&night_sky.values, &[(Param::Mix, 100.0), (Param::Shimmer, 0.0), (Param::Sub, 0.0)], &input);
    let from = frames(4.0, SR);
    let change =
        |lo: f32, hi: f32| band_energy_db(&with, from, lo, hi, SR) - band_energy_db(&without, from, lo, hi, SR);
    let (warmth, body, top) = (change(62.0, 250.0), change(250.0, 1000.0), change(2000.0, 8000.0));
    assert!(
        warmth > 2.0 && body > -3.0 && top < 0.0,
        "warmth (62-250 Hz) {warmth:+.1} dB, body (250 Hz-1 kHz) {body:+.1} dB, top (2-8 kHz) {top:+.1} dB"
    );
    assert_eq!(night_sky.values[Param::Resonance as usize], 0.0); // no hidden resonant peak from the filter
}
