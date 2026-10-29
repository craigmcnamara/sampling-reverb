//! Onset detection, analysis, matching, the pool, retention, voices and quantize.

mod common;

use common::*;
use sampling_reverb_dsp::engine::{ClockState, EngineParams, SampleEngine, SLOTS};
use sampling_reverb_dsp::visual::SRSlotVisual;

const SR: f32 = 48000.0;

struct EngineRig {
    engine: SampleEngine,
    out_l: Buffer,
    out_r: Buffer,
    max_active_voices: i32,
}

impl EngineRig {
    fn new(p: EngineParams) -> EngineRig {
        let mut engine = SampleEngine::default();
        engine.prepare(SR);
        engine.set_params(&p);
        EngineRig { engine, out_l: Vec::new(), out_r: Vec::new(), max_active_voices: 0 }
    }
    fn default() -> EngineRig {
        EngineRig::new(EngineParams::default())
    }
    fn set(&mut self, p: EngineParams) {
        self.engine.set_params(&p);
    }
    fn run(&mut self, input: &[f32]) {
        for chunk in input.chunks(256) {
            let (mut l, mut r) = (vec![0.0; chunk.len()], vec![0.0; chunk.len()]);
            self.engine.process(chunk, chunk, &mut l, &mut r);
            self.engine.advance_life(chunk.len() as f32 / SR);
            self.max_active_voices = self.max_active_voices.max(self.engine.active_voice_count());
            self.out_l.extend(l);
            self.out_r.extend(r);
        }
    }
    fn wait(&mut self, seconds: f32) {
        self.run(&silence(seconds, SR));
    }
    fn visuals(&self) -> [SRSlotVisual; SLOTS] {
        let mut v = [SRSlotVisual::default(); SLOTS];
        self.engine.fill_visual(&mut v);
        v
    }
    /// First and last non-zero output frames from `start`.
    fn sounding_span(&self, start: usize) -> (usize, usize) {
        let first = (start..self.out_l.len()).find(|&i| self.out_l[i] != 0.0).unwrap_or(self.out_l.len());
        let last = (first..self.out_l.len()).rev().find(|&i| self.out_l[i] != 0.0).unwrap_or(0);
        (first, last)
    }
}

fn note(hz: f32) -> Buffer {
    note_for(hz, 0.3, 0.5)
}
fn note_for(hz: f32, seconds: f32, amp: f32) -> Buffer {
    tone_burst(hz, seconds, amp, SR)
}
fn hit() -> Buffer {
    noise_burst(0.2, 0.5, SR, 1)
}

/// A sustained note: 5 ms attack, then constant level, so the replayed sample's own shape is flat.
fn pad(hz: f32, seconds: f32) -> Buffer {
    let mut b = sine(hz, seconds, 0.4, SR);
    let ramp = frames(0.005, SR);
    for (i, v) in b.iter_mut().take(ramp).enumerate() {
        *v *= i as f32 / ramp as f32;
    }
    b
}

// ---- Analysis ------------------------------------------------------------------------------------

#[test]
fn detects_pitch_of_a_tone() {
    for hz in [110.0f32, 220.0, 440.0, 880.0] {
        let mut rig = EngineRig::default();
        rig.run(&note(hz));
        let f = rig.engine.last_features();
        assert_near!(f.midi, 69.0 + 12.0 * (hz / 440.0).log2(), 0.35);
        assert!(f.clarity > 0.8);
    }
}

#[test]
fn noise_is_unpitched_and_bright() {
    let mut rig = EngineRig::default();
    rig.run(&hit());
    let f = rig.engine.last_features();
    assert!(f.midi < 0.0 && f.clarity < 0.5 && f.bright > 0.7);
}

#[test]
fn low_tones_are_darker_than_high_tones() {
    let (mut low, mut high) = (EngineRig::default(), EngineRig::default());
    low.run(&note(110.0));
    high.run(&note(1000.0));
    assert!(low.engine.last_features().bright < high.engine.last_features().bright);
    assert!(low.engine.last_features().bright < 0.3);
}

#[test]
fn map_position_uses_brightness_and_chroma() {
    let mut rig = EngineRig::default();
    rig.run(&note(440.0)); // A, chroma 9
    let f = rig.engine.last_features();
    assert_near!(f.x, f.bright, 1e-6);
    assert_near!(f.y, 9.5 / 12.0, 0.03);
}

// ---- Capture -------------------------------------------------------------------------------------

#[test]
fn silence_captures_nothing() {
    let mut rig = EngineRig::default();
    rig.wait(3.0);
    assert_eq!(rig.engine.pool_count(), 0);
    assert_eq!(peak(&rig.out_l), 0.0);
}

#[test]
fn novel_sound_is_captured_after_length() {
    let mut rig = EngineRig::new(EngineParams { length_sec: 0.5, ..Default::default() });
    rig.run(&note(220.0));
    rig.wait(0.1);
    assert_eq!(rig.engine.pool_count(), 0); // still recording (0.4 s elapsed)
    rig.wait(0.3);
    assert_eq!(rig.engine.pool_count(), 1);
    assert_eq!(rig.engine.total_captures(), 1);
    assert_eq!(rig.engine.total_triggers(), 0);
}

#[test]
fn ignores_sounds_below_sensitivity() {
    let mut rig = EngineRig::new(EngineParams { sensitivity_db: -30.0, ..Default::default() });
    rig.run(&note_for(220.0, 0.3, 0.01)); // about -46 dBFS envelope
    rig.wait(1.5);
    assert_eq!(rig.engine.pool_count(), 0);
}

#[test]
fn distinct_sounds_fill_separate_slots() {
    let mut rig = EngineRig::default();
    rig.run(&note(220.0));
    rig.wait(1.0);
    rig.run(&hit());
    rig.wait(1.0);
    assert_eq!(rig.engine.pool_count(), 2);
    let v = rig.visuals();
    let active: Vec<_> = v.iter().filter(|s| s.active != 0).collect();
    assert_eq!(active.iter().filter(|s| s.clarity > 0.5).count(), 1);
    assert_eq!(active.iter().filter(|s| s.clarity <= 0.5).count(), 1);
}

// ---- Matching ------------------------------------------------------------------------------------

#[test]
fn similar_sound_triggers_instead_of_capturing() {
    let mut rig = EngineRig::default();
    rig.run(&note(220.0));
    rig.wait(1.0);
    let before = rig.out_l.len();
    assert_eq!(peak(&rig.out_l), 0.0);
    rig.run(&note(220.0));
    rig.wait(0.5);
    assert_eq!(rig.engine.total_triggers(), 1);
    assert_eq!(rig.engine.pool_count(), 1);
    assert!(rms(&rig.out_l, before, rig.out_l.len()) > 1e-3);
}

#[test]
fn octave_matches_as_similar() {
    let mut rig = EngineRig::new(EngineParams { harmony: 0.0, ..Default::default() });
    rig.run(&note(220.0));
    rig.wait(1.0);
    rig.run(&note(440.0));
    rig.wait(1.0);
    assert_eq!(rig.engine.total_triggers(), 1);
}

#[test]
fn harmony_controls_whether_a_fifth_matches() {
    for harmony in [0.0, 1.0] {
        let mut rig = EngineRig::new(EngineParams { harmony, ..Default::default() });
        rig.run(&note(220.0));
        rig.wait(1.0);
        rig.run(&note(330.0)); // a fifth above
        rig.wait(1.0);
        let (triggers, pool) = if harmony == 0.0 { (0, 2) } else { (1, 1) };
        assert_eq!(rig.engine.total_triggers(), triggers, "harmony {harmony}");
        assert_eq!(rig.engine.pool_count(), pool, "harmony {harmony}");
    }
}

#[test]
fn match_zero_is_strict() {
    let mut rig = EngineRig::new(EngineParams { match_tolerance: 0.0, ..Default::default() });
    rig.run(&note(220.0));
    rig.wait(1.0);
    rig.run(&hit());
    rig.wait(1.0);
    assert_eq!(rig.engine.total_triggers(), 0);
}

#[test]
fn focus_biases_matching_towards_the_crosshair() {
    // Same sound, but the stored point is far from the focus and focus is at full strength.
    let replay = |focus_x: f32, focus_y: f32| {
        let mut rig = EngineRig::new(EngineParams {
            focus_amount: 1.0,
            focus_x,
            focus_y,
            match_tolerance: 0.25,
            ..Default::default()
        });
        rig.run(&note(220.0)); // sits near x≈0.2, y≈0.8
        rig.wait(1.0);
        rig.run(&note(220.0));
        rig.wait(0.5);
        rig.engine.total_triggers()
    };
    assert_eq!(replay(1.0, 0.0), 0);
    assert_eq!(replay(0.2, 0.8), 1);
}

// ---- Retention -----------------------------------------------------------------------------------

#[test]
fn low_retention_discards_unplayed_samples() {
    let mut rig = EngineRig::new(EngineParams { retention: 0.0, ..Default::default() }); // 3 s lifetime
    rig.run(&note(220.0));
    rig.wait(1.0);
    assert_eq!(rig.engine.pool_count(), 1);
    rig.wait(3.5);
    assert_eq!(rig.engine.pool_count(), 0);
}

#[test]
fn full_retention_keeps_samples_forever() {
    let mut rig = EngineRig::new(EngineParams { retention: 1.0, ..Default::default() });
    rig.run(&note(220.0));
    rig.wait(60.0);
    assert_eq!(rig.engine.pool_count(), 1);
}

#[test]
fn playing_a_sample_extends_its_life() {
    let mut rig = EngineRig::new(EngineParams { retention: 0.0, ..Default::default() }); // 3 s lifetime
    rig.run(&note(220.0)); // t=0: captured at ~0.8 s, would die at ~3.8 s
    rig.wait(0.7);
    rig.run(&hit()); // t=1.0: captured at ~1.8 s, dies at ~4.8 s
    rig.wait(1.2);
    rig.run(&note(220.0)); // t=2.5: retrigger extends the tone's life to ~5.7 s
    rig.wait(2.2); // t≈5.0
    assert_eq!(rig.engine.pool_count(), 1);
    for s in rig.visuals().iter().filter(|s| s.active != 0) {
        assert!(s.clarity > 0.5, "the survivor is the replayed tone");
    }
}

#[test]
fn full_pool_replaces_the_shortest_lived_slot() {
    let mut rig = EngineRig::new(EngineParams {
        match_tolerance: 0.0, // everything is novel
        length_sec: 0.1,
        retention: 1.0,
        ..Default::default()
    });
    // A chromatic run: neighbouring semitones never match, octaves differ in brightness.
    for i in 0..SLOTS + 4 {
        rig.run(&note_for(110.0 * 2f32.powf(i as f32 / 12.0), 0.2, 0.5));
        rig.wait(0.1);
    }
    assert_eq!(rig.engine.pool_count(), SLOTS as i32);
    assert_eq!(rig.engine.total_captures(), SLOTS as u64 + 4);
}

// ---- Freeze --------------------------------------------------------------------------------------

#[test]
fn freeze_blocks_capture_and_pauses_aging() {
    let mut p = EngineParams { retention: 0.0, ..Default::default() };
    let mut rig = EngineRig::new(p);
    rig.run(&note(220.0));
    rig.wait(1.0);
    p.freeze = true;
    rig.set(p);
    rig.run(&hit());
    rig.wait(10.0);
    assert_eq!(rig.engine.pool_count(), 1);
    assert_eq!(rig.engine.total_captures(), 1);
    rig.run(&note(220.0)); // matching still works while frozen
    rig.wait(0.5);
    assert_eq!(rig.engine.total_triggers(), 1);
}

// ---- Voices --------------------------------------------------------------------------------------

#[test]
fn density_caps_simultaneous_voices() {
    for density in [1, 4] {
        let mut rig = EngineRig::new(EngineParams { density, length_sec: 1.5, ..Default::default() });
        rig.run(&note(220.0));
        rig.wait(1.5);
        for _ in 0..5 {
            rig.run(&note_for(220.0, 0.2, 0.5));
        }
        rig.wait(2.0);
        assert!(rig.engine.total_triggers() >= 4);
        assert!(rig.max_active_voices <= density);
        if density > 1 {
            assert!(rig.max_active_voices > 1);
        }
    }
}

#[test]
fn visual_state_reports_triggers_and_levels() {
    let mut rig = EngineRig::default();
    rig.run(&note(220.0));
    rig.wait(1.0);
    let before = rig.visuals();
    let slot = (0..SLOTS).rev().find(|&s| before[s].active != 0).expect("a captured slot");
    assert_eq!(before[slot].generation, 1);
    assert_eq!(before[slot].trigger_count, 0);
    assert_eq!(before[slot].level, 0.0);
    assert!((0.9..=1.0).contains(&before[slot].life));

    rig.run(&note_for(220.0, 0.15, 0.5));
    let during = rig.visuals();
    assert_eq!(during[slot].trigger_count, 1);
    assert!(during[slot].level > 0.1);
    assert!(during[slot].life > before[slot].life);
}

#[test]
fn voice_output_is_the_recorded_sound() {
    let mut rig = EngineRig::new(EngineParams { length_sec: 0.5, ..Default::default() });
    rig.run(&note(440.0));
    rig.wait(1.0);
    let start = rig.out_l.len();
    rig.run(&note(440.0));
    // The replayed sample (after the analysis window) is a 440 Hz tone too.
    let from = start + 2048 + frames(0.05, SR);
    assert_near!(zero_crossing_hz(&rig.out_l, from, from + frames(0.2, SR), SR), 440.0, 10.0);
}

// ---- Attack --------------------------------------------------------------------------------------

/// Replays a captured pad and returns voice RMS just after the voice starts vs. 400–450 ms later.
fn voice_onset_levels(attack_sec: f32) -> (f32, f32) {
    let mut rig = EngineRig::new(EngineParams { length_sec: 1.0, attack_sec, ..Default::default() });
    rig.run(&pad(220.0, 0.9));
    rig.wait(1.0);
    let start = rig.out_l.len();
    rig.run(&pad(220.0, 0.9));
    let voice = (start..rig.out_l.len()).find(|&i| rig.out_l[i] != 0.0).expect("the voice started");
    (
        rms(&rig.out_l, voice, voice + frames(0.03, SR)),
        rms(&rig.out_l, voice + frames(0.4, SR), voice + frames(0.45, SR)),
    )
}

#[test]
fn short_attack_plays_samples_at_full_level_immediately() {
    let (early, late) = voice_onset_levels(0.001);
    assert!(early > 0.5 * late);
}

#[test]
fn long_attack_swells_samples_in() {
    let (early, late) = voice_onset_levels(0.5);
    assert!(early < 0.1 * late, "early {early}, late {late}");
    assert!(late > 1e-3);
}

// ---- Long samples --------------------------------------------------------------------------------

#[test]
fn captures_and_replays_long_samples() {
    let mut rig = EngineRig::new(EngineParams { length_sec: 15.0, attack_sec: 0.001, ..Default::default() });
    rig.run(&pad(220.0, 14.0)); // a long sustained note
    rig.wait(2.0);
    assert_eq!(rig.engine.pool_count(), 1);
    let start = rig.out_l.len();
    rig.run(&note(220.0));
    rig.wait(14.0);
    // The replay is still sounding 12 s in: the whole long take was kept.
    let at = start + frames(12.0, SR);
    assert!(rms(&rig.out_l, at, at + frames(0.5, SR)) > 0.05);
    assert_near!(zero_crossing_hz(&rig.out_l, at, at + frames(0.5, SR), SR), 220.0, 5.0);
}

#[test]
fn long_lengths_still_capture_overlapping_phrases() {
    // With a 5 s Length, three distinct notes half a second apart all get recorded.
    let mut rig = EngineRig::new(EngineParams { length_sec: 5.0, match_tolerance: 0.0, ..Default::default() });
    for hz in [220.0, 330.0, 494.0] {
        rig.run(&note_for(hz, 0.3, 0.5));
        rig.wait(0.2);
    }
    rig.wait(6.0);
    assert_eq!(rig.engine.pool_count(), 3);
    assert_eq!(rig.engine.total_captures(), 3);
}

#[test]
fn captured_samples_fade_in_and_out_without_clicks() {
    let mut rig = EngineRig::new(EngineParams { length_sec: 0.5, attack_sec: 0.0005, ..Default::default() });
    rig.run(&pad(220.0, 0.8)); // still loud when the 0.5 s capture ends
    rig.wait(1.0);
    let start = rig.out_l.len();
    rig.run(&pad(220.0, 0.8));
    rig.wait(0.5);
    let (first, last) = rig.sounding_span(start);
    assert!(last > first);
    let middle = rms(&rig.out_l, (first + last) / 2, (first + last) / 2 + 2400);
    assert!(rig.out_l[first].abs() < 0.05 * middle, "starts from silence");
    assert!(rig.out_l[last].abs() < 0.05 * middle, "ends at silence, no cut-off click");
}

#[test]
fn one_sustained_note_is_captured_once() {
    let mut rig = EngineRig::new(EngineParams { length_sec: 5.0, ..Default::default() });
    rig.run(&pad(220.0, 3.0));
    rig.wait(3.0);
    assert_eq!(rig.engine.total_captures(), 1);
    assert_eq!(rig.engine.pool_count(), 1);
}

#[test]
fn repeating_a_note_during_its_long_capture_does_not_duplicate_it() {
    let mut rig = EngineRig::new(EngineParams { length_sec: 10.0, ..Default::default() });
    for _ in 0..3 {
        rig.run(&note_for(220.0, 0.3, 0.5));
        rig.wait(0.7);
    }
    rig.wait(9.0);
    assert_eq!(rig.engine.total_captures(), 1);
}

// ---- Smooth endings ------------------------------------------------------------------------------

#[test]
fn density_steals_voices_with_a_gentle_release() {
    // Two long samples at different pitches; with Density 1, triggering the second must fade the
    // first out gently rather than chopping it.
    let mut rig = EngineRig::new(EngineParams { length_sec: 3.0, density: 1, attack_sec: 0.001, ..Default::default() });
    rig.run(&pad(220.0, 2.5));
    rig.wait(1.0);
    rig.run(&pad(660.0, 2.5));
    rig.wait(1.0);
    assert_eq!(rig.engine.pool_count(), 2);

    rig.run(&note(220.0)); // plays the 220 Hz sample
    rig.wait(0.5);
    let before = rig.engine.total_triggers();
    let mut steal = 0;
    for chunk in note(660.0).chunks(256) {
        rig.run(chunk); // find the block that triggers it
        if rig.engine.total_triggers() > before {
            steal = rig.out_l.len();
            break;
        }
    }
    assert!(steal > 0);
    rig.wait(0.5);

    let held = tone_level(&rig.out_l, steal - frames(0.06, SR), steal - frames(0.01, SR), 220.0, SR);
    let after = tone_level(&rig.out_l, steal + frames(0.02, SR), steal + frames(0.07, SR), 220.0, SR);
    assert!(held > 0.05);
    assert!(after > 0.3 * held, "220 Hz level before steal {held}, 20-70 ms after {after}");
}

#[test]
fn samples_cut_mid_note_fade_out_gradually() {
    // A note still sounding when Length ends: the replay should taper over a meaningful stretch, not
    // drop out in the final few milliseconds.
    let mut rig = EngineRig::new(EngineParams { length_sec: 1.0, attack_sec: 0.001, ..Default::default() });
    rig.run(&pad(220.0, 1.5));
    rig.wait(1.0);
    let start = rig.out_l.len();
    rig.run(&note(220.0));
    rig.wait(1.5);
    let (first, last) = rig.sounding_span(start);
    assert!(last > first + frames(0.8, SR));
    let mid = (first + last) / 2;
    let middle = rms(&rig.out_l, mid, mid + frames(0.05, SR));
    let near_end = rms(&rig.out_l, last - frames(0.15, SR), last - frames(0.10, SR));
    assert!(near_end < 0.7 * middle, "middle {middle}, 100-150 ms before the end {near_end}");
}

// ---- Quantize ------------------------------------------------------------------------------------

const BEATS_PER_FRAME_120: f64 = 120.0 / (60.0 * SR as f64);

/// Feeds the engine one frame at a time from a known clock and reports exact trigger frames.
struct QuantizeRig {
    engine: SampleEngine,
    clock: u64,
}

impl QuantizeRig {
    fn new(quantize: bool, grid_beats: f64, bar_origin: f64) -> QuantizeRig {
        let mut engine = SampleEngine::default();
        engine.prepare(SR);
        engine.set_params(&EngineParams { length_sec: 0.5, quantize, grid_beats, ..Default::default() });
        // The engine extrapolates from here.
        engine.set_clock(ClockState { beat: 0.0, beats_per_frame: BEATS_PER_FRAME_120, bar_origin });
        let mut rig = QuantizeRig { engine, clock: 0 };
        // Capture a note, then let it land in the pool.
        rig.feed(&note(440.0));
        rig.feed(&silence(1.0, SR));
        rig
    }

    /// Returns the frame at which the next trigger fired, if any.
    fn feed(&mut self, input: &[f32]) -> Option<u64> {
        let before = self.engine.total_triggers();
        let mut fired = None;
        let (mut l, mut r) = ([0f32], [0f32]);
        for x in input {
            self.engine.process(std::slice::from_ref(x), std::slice::from_ref(x), &mut l, &mut r);
            if fired.is_none() && self.engine.total_triggers() != before {
                fired = Some(self.clock);
            }
            self.clock += 1;
        }
        fired
    }

    /// Plays the note again `offset` frames after the next whole second: (trigger frame, onset frame).
    fn replay(&mut self, offset: u64) -> (u64, u64) {
        let target = (self.clock / 48000 + 1) * 48000 + offset;
        self.feed(&vec![0.0; (target - self.clock) as usize]);
        let onset = self.clock;
        (self.feed(&note(440.0)).expect("triggered"), onset)
    }
}

#[test]
fn unquantized_trigger_fires_right_after_analysis() {
    let mut rig = QuantizeRig::new(false, 0.5, 0.0);
    assert_eq!(rig.engine.pool_count(), 1);
    let (fired, onset) = rig.replay(3000);
    assert!(fired - onset < 2048 + 64);
}

#[test]
fn quantize_holds_the_trigger_until_the_next_grid_point() {
    // 1/8 notes at 120 BPM are 12000 frames apart.
    let mut rig = QuantizeRig::new(true, 0.5, 0.0);
    assert_eq!(rig.engine.pool_count(), 1);
    for offset in [3000, 9500, 13000] {
        let (fired, onset) = rig.replay(offset);
        assert!(fired % 12000 <= 1, "offset {offset}: fired at {fired}");
        assert!(fired > onset + 2048);
        assert!(fired <= onset + 2048 + 12000);
    }
}

#[test]
fn quantize_supports_triplets_and_follows_the_bar_origin() {
    // 1/8 triplets (1/3 beat = 8000 frames), with the bar line a quarter beat (6000 frames) in.
    let mut rig = QuantizeRig::new(true, 1.0 / 3.0, 0.25);
    let (fired, onset) = rig.replay(1000);
    assert!((fired as i64 - 6000).rem_euclid(8000) <= 1, "fired at {fired}");
    assert!(fired > onset + 2048);
}

#[test]
fn quantized_slot_is_not_triggered_twice_for_one_grid_point() {
    // A whole bar (4 beats = 96000 frames): a second matching note while the first waits must not
    // queue the same slot again.
    let mut rig = QuantizeRig::new(true, 4.0, 0.0);
    let before = rig.engine.total_triggers();
    let mut two_notes = note_for(440.0, 0.2, 0.5);
    append(&mut two_notes, &silence(0.15, SR));
    append(&mut two_notes, &note_for(440.0, 0.2, 0.5));
    append(&mut two_notes, &silence(2.0, SR));
    rig.feed(&two_notes);
    assert_eq!(rig.engine.total_triggers() - before, 1);
}

// ---- Gain staging --------------------------------------------------------------------------------

#[test]
fn overlapping_replays_do_not_stack_up_in_level() {
    // One long noise sample, replayed every 0.25 s: up to four copies overlap. Their level should
    // stay near a single replay's instead of adding up (+6 dB for four).
    let mut rig = EngineRig::new(EngineParams { length_sec: 1.5, density: 8, attack_sec: 0.001, ..Default::default() });
    // Flat (not decaying) noise, so earlier replays are still as loud as the newest one.
    let mut rng = sampling_reverb_dsp::util::Rng::default();
    let mut sustained: Buffer = (0..frames(1.6, SR)).map(|_| 0.3 * rng.bipolar()).collect();
    for (i, v) in sustained.iter_mut().take(240).enumerate() {
        *v *= i as f32 / 240.0;
    }
    rig.run(&sustained);
    rig.wait(1.0);
    assert_eq!(rig.engine.pool_count(), 1);
    let start = rig.out_l.len();
    rig.run(&noise_burst(0.2, 0.3, SR, 5));
    rig.wait(0.05);
    let single = rms(&rig.out_l, start + frames(0.08, SR), start + frames(0.2, SR));
    for _ in 0..3 {
        rig.run(&noise_burst(0.2, 0.3, SR, 5));
        rig.wait(0.05);
    }
    let end = rig.out_l.len();
    let stacked = rms(&rig.out_l, end - frames(0.15, SR), end);
    assert!(rig.engine.total_triggers() >= 3);
    assert!(db(stacked) - db(single) < 3.0, "overlapping replays {:+.1} dB louder than one", db(stacked) - db(single));
}
