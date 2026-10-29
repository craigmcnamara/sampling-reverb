//! Behaviour tests for the Rust DSP core. Bit-for-bit equivalence with the C++ reference is checked
//! separately by `make test-parity`.

mod common;

use common::*;
use sampling_reverb_dsp::engine::{ClockState, EngineParams, SampleEngine, SLOTS};
use sampling_reverb_dsp::ffi::*;
use sampling_reverb_dsp::kernel::{Kernel, SRHostTime};
use sampling_reverb_dsp::params::*;
use sampling_reverb_dsp::reverb::{FdnReverb, PitchShifter, ReverbParams};
use sampling_reverb_dsp::triple_buffer::TripleBuffer;
use sampling_reverb_dsp::util::*;
use sampling_reverb_dsp::visual::{SRSlotVisual, SR_MAX_SLOTS, SR_SCOPE_POINTS};

const SR: f32 = 48000.0;

fn near(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

// ---- Building blocks -----------------------------------------------------------------------------

#[test]
fn next_pow2_rounds_up() {
    assert_eq!(next_pow2(1), 1);
    assert_eq!(next_pow2(3), 4);
    assert_eq!(next_pow2(1024), 1024);
    assert_eq!(next_pow2(1025), 2048);
}

#[test]
fn safe_limit_is_transparent_below_unity_and_bounded() {
    for x in [-1.0, -0.5, 0.0, 0.3, 1.0] {
        assert_eq!(safe_limit(x), x);
    }
    for x in [1.5f32, 10.0, 1e30, -1e30] {
        assert!(safe_limit(x).abs() <= 2.0 && safe_limit(x).signum() == x.signum());
    }
}

#[test]
fn delay_line_reads_integer_and_fractional_delays() {
    let mut d = DelayLine::default();
    d.allocate(16);
    for i in 0..10 {
        d.write(i as f32);
    }
    assert_eq!(d.read_int(0), 9.0);
    assert_eq!(d.read_int(3), 6.0);
    assert!(near(d.read(2.5), 6.5, 1e-6));
    // Delays beyond the buffer wrap around instead of reading out of bounds.
    let _ = d.read(1e6);
    let _ = d.read_int(u32::MAX);
}

#[test]
fn allpass_preserves_impulse_energy() {
    let mut a = Allpass::default();
    a.allocate(37);
    a.g = 0.6;
    let mut energy = 0.0f64;
    for i in 0..20000 {
        let y = a.process(if i == 0 { 1.0 } else { 0.0 });
        energy += (y * y) as f64;
    }
    assert!((energy - 1.0).abs() < 1e-3, "energy {energy}");
}

#[test]
fn svf_low_and_high_pass_split_the_spectrum() {
    let mut f = Svf::default();
    f.set(1000.0, 0.707, SR);
    let (mut lo_low, mut lo_high) = (Vec::new(), Vec::new());
    for x in sine(100.0, 0.5, 1.0, SR) {
        let (lp, hp) = f.process(x);
        lo_low.push(lp);
        lo_high.push(hp);
    }
    let n = lo_low.len();
    assert!(rms(&lo_low, n / 2, n) > 0.6);
    assert!(rms(&lo_high, n / 2, n) < 0.02);
}

#[test]
fn half_floats_round_trip_exactly_and_round_to_nearest_even() {
    for bits in 0..=u16::MAX {
        let h = F16(bits);
        let f = h.to_f32();
        if f.is_nan() {
            assert!(F16::from_f32(f).to_f32().is_nan());
        } else {
            assert_eq!(F16::from_f32(f), h, "bits {bits:#06x}");
        }
    }
    assert_eq!(F16::from_f32(1.0 + 1.0 / 2048.0).to_f32(), 1.0); // tie → even
    assert_eq!(F16::from_f32(1.0 + 3.0 / 2048.0).to_f32(), 1.0 + 2.0 / 1024.0); // tie → even (up)
    assert_eq!(F16::from_f32(65520.0).to_f32(), f32::INFINITY);
    assert_eq!(F16::from_f32(65519.0).to_f32(), 65504.0);
    assert_eq!(F16::from_f32(-0.0).0, 0x8000);
}

#[test]
fn triple_buffer_never_tears_under_concurrency() {
    let tb = std::sync::Arc::new(TripleBuffer::<[u64; 32]>::default());
    let writer = {
        let tb = tb.clone();
        std::thread::spawn(move || {
            for v in 1..=200_000u64 {
                unsafe {
                    *tb.back_buffer() = [v; 32];
                    tb.publish();
                }
            }
        })
    };
    let mut last = 0;
    while !writer.is_finished() {
        let v = unsafe { *tb.read() };
        assert!(v.iter().all(|x| *x == v[0]), "torn read");
        assert!(v[0] >= last, "went backwards");
        last = v[0];
    }
    writer.join().unwrap();
    assert_eq!(unsafe { tb.read()[0] }, 200_000);
}

// ---- Reverb --------------------------------------------------------------------------------------

#[test]
fn pitch_shifter_shifts_up_an_octave() {
    let mut p = PitchShifter::default();
    p.prepare(SR);
    p.set_semitones(12.0);
    let out: Vec<f32> = sine(220.0, 1.0, 0.5, SR).into_iter().map(|x| p.process(x)).collect();
    let n = out.len();
    assert!(tone_level(&out, n / 2, n, 440.0, SR) > 4.0 * tone_level(&out, n / 2, n, 220.0, SR));
}

fn reverb_impulse(params: ReverbParams, seconds: f32) -> (Buffer, Buffer) {
    let mut r = FdnReverb::default();
    r.prepare(SR);
    r.set_params(&params);
    let n = frames(seconds, SR);
    let mut input = vec![0.0; n];
    input[0] = 1.0;
    let (mut l, mut rr) = (vec![0.0; n], vec![0.0; n]);
    for i in (0..n).step_by(256) {
        let e = (i + 256).min(n);
        r.process(&input[i..e], &input[i..e], &mut l[i..e], &mut rr[i..e]);
    }
    (l, rr)
}

#[test]
fn reverb_tail_is_decorrelated_stereo_and_decays() {
    let (l, r) = reverb_impulse(ReverbParams { decay_sec: 2.0, ..Default::default() }, 4.0);
    let early = rms(&l, frames(0.1, SR), frames(0.5, SR));
    let late = rms(&l, frames(3.0, SR), frames(3.5, SR));
    assert!(early > 1e-3);
    assert!(late < early * 0.05, "early {early} late {late}");
    let diff: f32 = l.iter().zip(&r).map(|(a, b)| (a - b).abs()).sum();
    assert!(diff > 1.0, "left and right should differ");
}

#[test]
fn reverb_shimmer_and_sub_never_run_away() {
    for interval in INTERVAL_SEMITONES {
        let params = ReverbParams {
            decay_sec: 30.0,
            size: 1.0,
            shimmer: 1.0,
            shimmer_semitones: interval,
            sub: 1.0,
            ..Default::default()
        };
        let (l, r) = reverb_impulse(params, 12.0);
        assert!(all_finite(&l) && all_finite(&r));
        assert!(peak(&l) < 4.0 && peak(&r) < 4.0, "interval {interval}: peak {}", peak(&l));
    }
}

#[test]
fn reverb_freeze_holds_the_tail() {
    let mut r = FdnReverb::default();
    r.prepare(SR);
    r.set_params(&ReverbParams { decay_sec: 1.0, ..Default::default() });
    let burst = noise_burst(0.3, 0.5, SR, 3);
    let (mut l, mut rr) = (vec![0.0; burst.len()], vec![0.0; burst.len()]);
    r.process(&burst, &burst, &mut l, &mut rr);
    r.set_params(&ReverbParams { decay_sec: 1.0, freeze: true, ..Default::default() });
    let n = frames(6.0, SR);
    let zeros = vec![0.0; n];
    let (mut l, mut rr) = (vec![0.0; n], vec![0.0; n]);
    r.process(&zeros, &zeros, &mut l, &mut rr);
    let start = rms(&l, frames(0.5, SR), frames(1.0, SR));
    let end = rms(&l, frames(5.5, SR), frames(6.0, SR));
    assert!(end > 0.5 * start, "frozen tail faded: {start} → {end}");
}

// ---- Sample engine -------------------------------------------------------------------------------

struct EngineRig {
    engine: SampleEngine,
    out_l: Buffer,
    out_r: Buffer,
}

impl EngineRig {
    fn new(p: EngineParams) -> EngineRig {
        let mut engine = SampleEngine::default();
        engine.prepare(SR);
        engine.set_params(&p);
        EngineRig { engine, out_l: Vec::new(), out_r: Vec::new() }
    }
    fn run(&mut self, input: &[f32]) {
        for chunk in input.chunks(256) {
            let (mut l, mut r) = (vec![0.0; chunk.len()], vec![0.0; chunk.len()]);
            self.engine.process(chunk, chunk, &mut l, &mut r);
            self.engine.advance_life(chunk.len() as f32 / SR);
            self.out_l.extend(l);
            self.out_r.extend(r);
        }
    }
    fn wait(&mut self, seconds: f32) {
        self.run(&silence(seconds, SR));
    }
}

fn note(hz: f32) -> Buffer {
    tone_burst(hz, 0.3, 0.5, SR)
}

#[test]
fn engine_detects_pitch() {
    for hz in [110.0f32, 220.0, 440.0, 880.0] {
        let mut rig = EngineRig::new(EngineParams::default());
        rig.run(&note(hz));
        let f = rig.engine.last_features();
        let expected = 69.0 + 12.0 * (hz / 440.0).log2();
        assert!(near(f.midi, expected, 0.35), "{hz} Hz: midi {}", f.midi);
        assert!(f.clarity > 0.8);
    }
}

#[test]
fn engine_noise_is_unpitched_and_bright() {
    let mut rig = EngineRig::new(EngineParams::default());
    rig.run(&noise_burst(0.2, 0.5, SR, 1));
    let f = rig.engine.last_features();
    assert!(f.midi < 0.0 && f.clarity < 0.5 && f.bright > 0.7);
}

#[test]
fn engine_captures_new_sounds_and_replays_similar_ones() {
    let mut rig = EngineRig::new(EngineParams::default());
    rig.run(&note(440.0));
    rig.wait(1.0);
    assert_eq!(rig.engine.pool_count(), 1);
    assert_eq!(rig.engine.total_triggers(), 0);
    let start = rig.out_l.len();
    rig.run(&note(440.0));
    rig.wait(0.5);
    assert_eq!(rig.engine.total_triggers(), 1);
    assert_eq!(rig.engine.pool_count(), 1, "a match plays, it doesn't capture");
    let from = start + 2048 + frames(0.05, SR);
    assert!(near(zero_crossing_hz(&rig.out_l, from, from + frames(0.2, SR), SR), 440.0, 10.0));
}

#[test]
fn engine_harmony_controls_whether_a_fifth_matches() {
    let fifth = |harmony: f32| {
        let mut rig = EngineRig::new(EngineParams { harmony, ..Default::default() });
        rig.run(&note(220.0));
        rig.wait(1.0);
        rig.run(&note(330.0));
        rig.wait(0.2);
        rig.engine.total_triggers()
    };
    assert_eq!(fifth(0.0), 0);
    assert_eq!(fifth(1.0), 1);
}

#[test]
fn engine_low_retention_discards_unplayed_samples() {
    let mut rig = EngineRig::new(EngineParams { retention: 0.0, ..Default::default() });
    rig.run(&note(440.0));
    rig.wait(1.0);
    assert_eq!(rig.engine.pool_count(), 1);
    rig.wait(3.0);
    assert_eq!(rig.engine.pool_count(), 0);
}

#[test]
fn engine_density_caps_simultaneous_voices() {
    let mut rig = EngineRig::new(EngineParams { density: 2, length_sec: 3.0, ..Default::default() });
    for hz in [220.0, 330.0, 495.0, 660.0] {
        rig.run(&note(hz));
        rig.wait(3.5);
    }
    let mut most = 0;
    for _ in 0..3 {
        for hz in [220.0, 330.0, 495.0, 660.0] {
            rig.run(&note(hz));
            most = most.max(rig.engine.active_voice_count());
        }
    }
    assert!(most <= 2, "{most} voices");
}

#[test]
fn engine_visuals_report_triggers() {
    let mut rig = EngineRig::new(EngineParams::default());
    rig.run(&note(440.0));
    rig.wait(1.0);
    rig.run(&note(440.0));
    let mut v = [SRSlotVisual::default(); SLOTS];
    rig.engine.fill_visual(&mut v);
    let active: Vec<_> = v.iter().filter(|s| s.active != 0).collect();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].trigger_count, 1);
}

#[test]
fn engine_quantize_holds_the_trigger_until_the_next_grid_point() {
    // 1/8 notes at 120 BPM are 12000 frames apart.
    let mut engine = SampleEngine::default();
    engine.prepare(SR);
    engine.set_params(&EngineParams { length_sec: 0.5, quantize: true, grid_beats: 0.5, ..Default::default() });
    engine.set_clock(ClockState { beat: 0.0, beats_per_frame: 120.0 / (60.0 * SR as f64), bar_origin: 0.0 });
    let mut clock = 0u64;
    let feed = |engine: &mut SampleEngine, input: &[f32], clock: &mut u64| -> Option<u64> {
        let before = engine.total_triggers();
        let mut fired = None;
        let (mut l, mut r) = ([0f32], [0f32]);
        for x in input {
            engine.process(std::slice::from_ref(x), std::slice::from_ref(x), &mut l, &mut r);
            if fired.is_none() && engine.total_triggers() != before {
                fired = Some(*clock);
            }
            *clock += 1;
        }
        fired
    };
    feed(&mut engine, &note(440.0), &mut clock);
    feed(&mut engine, &silence(1.0, SR), &mut clock);
    for offset in [3000u64, 9500, 13000] {
        let target = (clock / 48000 + 1) * 48000 + offset;
        feed(&mut engine, &vec![0.0; (target - clock) as usize], &mut clock);
        let onset = clock;
        let fired = feed(&mut engine, &note(440.0), &mut clock).expect("triggered");
        assert!(fired % 12000 <= 1, "offset {offset}: fired at {fired}");
        assert!(fired > onset + 2048 && fired <= onset + 2048 + 12000);
    }
}

// ---- Kernel --------------------------------------------------------------------------------------

struct KernelRig {
    kernel: Kernel,
    host: SRHostTime,
    out_l: Buffer,
    out_r: Buffer,
    block: usize,
}

impl KernelRig {
    fn new(sr: f32) -> KernelRig {
        let kernel = Kernel::new();
        unsafe { kernel.prepare(sr as f64) };
        kernel.set_parameter(Param::Length as u64, 0.8);
        KernelRig { kernel, host: SRHostTime::default(), out_l: Vec::new(), out_r: Vec::new(), block: 256 }
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
}

#[test]
fn kernel_defaults_match_the_parameter_table() {
    let k = Kernel::new();
    for (i, spec) in PARAM_SPECS.iter().enumerate() {
        assert_eq!(spec.address as usize, i, "specs are ordered by address");
        assert_eq!(k.get_parameter(i as u64), spec.def);
    }
    k.set_parameter(PARAM_COUNT as u64 + 5, 1.0);
    assert_eq!(k.get_parameter(PARAM_COUNT as u64 + 5), 0.0);
}

#[test]
fn kernel_bypass_passes_input_through_exactly() {
    let mut rig = KernelRig::new(SR);
    rig.kernel.set_bypass(true);
    let input = noise_burst(0.5, 0.5, SR, 1);
    rig.run(&input);
    assert_eq!(rig.out_l, input);
    assert_eq!(rig.out_r, input);
}

#[test]
fn kernel_mix_zero_is_dry() {
    let mut rig = KernelRig::new(SR);
    rig.set(Param::Mix, 0.0);
    let input = strum(1.0, 0.5, SR);
    rig.run(&input);
    for (a, b) in rig.out_l.iter().zip(&input) {
        assert!((a - b).abs() < 1e-6);
    }
}

#[test]
fn kernel_in_place_matches_separate_buffers_and_block_size_does_not_matter() {
    let mut input = strum(1.0, 0.5, SR);
    append(&mut input, &note(440.0));
    append(&mut input, &silence(1.0, SR));
    let mut reference = KernelRig::new(SR);
    reference.run(&input);

    for block in [1usize, 7, 64, 1000, 4096] {
        let k = Kernel::new();
        unsafe { k.prepare(SR as f64) };
        k.set_parameter(Param::Length as u64, 0.8);
        let (mut l, mut r) = (input.clone(), input.clone());
        let host = SRHostTime::default();
        let mut i = 0;
        while i < l.len() {
            let n = block.min(l.len() - i);
            // In place, as the AU does: the outputs are the input buffers.
            unsafe {
                k.process_raw(
                    &host,
                    l.as_ptr().add(i),
                    r.as_ptr().add(i),
                    l.as_mut_ptr().add(i),
                    r.as_mut_ptr().add(i),
                    n,
                )
            };
            i += n;
        }
        assert_eq!(l, reference.out_l, "block {block}");
        assert_eq!(r, reference.out_r, "block {block}");
    }
}

#[test]
fn kernel_mono_in_place_input_is_read_before_it_is_overwritten() {
    let input = strum(1.0, 0.5, SR);
    let mut reference = KernelRig::new(SR);
    reference.run(&input);
    let k = Kernel::new();
    unsafe { k.prepare(SR as f64) };
    k.set_parameter(Param::Length as u64, 0.8);
    let mut l = input.clone();
    let mut r = vec![0.0; l.len()];
    // inL == inR == outL (mono, in place), outR separate.
    unsafe { k.process_raw(&SRHostTime::default(), l.as_ptr(), l.as_ptr(), l.as_mut_ptr(), r.as_mut_ptr(), l.len()) };
    assert_eq!(l, reference.out_l);
    assert_eq!(r, reference.out_r);
}

#[test]
fn kernel_handles_every_common_sample_rate() {
    for sr in [44100.0f32, 48000.0, 88200.0, 96000.0, 176400.0, 192000.0] {
        let mut rig = KernelRig::new(sr);
        let mut input = tone_burst(330.0, 0.3, 0.5, sr);
        append(&mut input, &silence(0.5, sr));
        append(&mut input, &tone_burst(330.0, 0.3, 0.5, sr));
        append(&mut input, &silence(0.5, sr));
        rig.run(&input);
        assert!(all_finite(&rig.out_l) && all_finite(&rig.out_r), "{sr}");
        assert!(peak(&rig.out_l) > 0.01, "{sr}");
    }
}

#[test]
fn kernel_survives_random_parameter_abuse() {
    let mut rng = Rng::default();
    let mut rig = KernelRig::new(SR);
    let input = {
        let mut b = Buffer::new();
        for i in 0..20 {
            append(
                &mut b,
                &if i % 3 == 0 {
                    noise_burst(0.2, 0.9, SR, i)
                } else {
                    tone_burst(100.0 + 50.0 * i as f32, 0.3, 0.9, SR)
                },
            );
        }
        b
    };
    for chunk in input.chunks(4800) {
        for spec in &PARAM_SPECS {
            // Anything in range, and now and then far outside it.
            let v = if rng.uniform() < 0.05 {
                rng.bipolar() * 1e6
            } else {
                spec.min + (spec.max - spec.min) * rng.uniform()
            };
            rig.kernel.set_parameter(spec.address as u64, v);
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
fn kernel_synced_pre_delay_follows_the_tempo() {
    let arrival = |params: &[(Param, f32)]| -> f32 {
        let mut rig = KernelRig::new(SR);
        rig.set(Param::Mix, 100.0);
        rig.set(Param::DryFeed, 100.0);
        rig.set(Param::SampleLevel, 0.0);
        for (p, v) in params {
            rig.set(*p, *v);
        }
        let mut input = silence(0.1, SR);
        let at = input.len();
        input.push(1.0);
        append(&mut input, &silence(2.0, SR));
        rig.run(&input);
        let i = (at..rig.out_l.len()).find(|&i| rig.out_l[i].abs() > 1e-4 || rig.out_r[i].abs() > 1e-4).unwrap();
        (i - at) as f32 / SR
    };
    let base = arrival(&[(Param::PreDelay, 0.0)]);
    let eighth = arrival(&[(Param::Sync, 1.0), (Param::PreDelayDiv, 7.0), (Param::Tempo, 120.0)]);
    let dotted = arrival(&[(Param::Sync, 1.0), (Param::PreDelayDiv, 11.0), (Param::Tempo, 60.0)]);
    assert!(near(eighth - base, 0.25, 0.002), "{}", eighth - base);
    assert!(near(dotted - base, 1.5, 0.002), "{}", dotted - base);
}

#[test]
fn kernel_quantize_follows_the_host_transport() {
    let mut rig = KernelRig::new(SR);
    rig.block = 16;
    rig.set(Param::Length, 0.5);
    rig.set(Param::Quantize, 1.0);
    rig.set(Param::Grid, 5.0); // 1/4
    let bpf = 100.0 / (60.0 * SR as f64);
    let mut frame = 0u64;
    let mut feed = |rig: &mut KernelRig, input: &[f32]| -> Option<u64> {
        let before = unsafe { rig.kernel.engine().total_triggers() };
        let mut fired = None;
        for chunk in input.chunks(16) {
            rig.host = SRHostTime {
                has_tempo: true,
                tempo: 100.0,
                has_position: true,
                beat: 10.4 + frame as f64 * bpf,
                downbeat: 8.0,
                bar_beats: 4.0,
                has_transport: true,
                playing: true,
            };
            rig.run(chunk);
            if fired.is_none() && unsafe { rig.kernel.engine().total_triggers() } != before {
                fired = Some(frame);
            }
            frame += chunk.len() as u64;
        }
        fired
    };
    feed(&mut rig, &note(440.0));
    feed(&mut rig, &silence(2.0, SR));
    let mut replay = note(440.0);
    append(&mut replay, &silence(1.0, SR));
    let fired = feed(&mut rig, &replay).expect("triggered");
    let beat = 10.4 + fired as f64 * bpf;
    assert!((beat - beat.round()).abs() < 16.0 * bpf, "fired at beat {beat}");
    assert!(rig.kernel.clock().from_host);
    assert_eq!(rig.kernel.clock().tempo, 100.0);
}

#[test]
fn kernel_visuals_and_clock_are_published() {
    let mut rig = KernelRig::new(SR);
    rig.set(Param::Tempo, 97.5);
    let mut input = note(440.0);
    append(&mut input, &silence(1.0, SR));
    rig.run(&input);
    let mut slots = [SRSlotVisual::default(); SR_MAX_SLOTS];
    let (mut l, mut r) = ([0f32; SR_SCOPE_POINTS], [0f32; SR_SCOPE_POINTS]);
    let summary = unsafe { rig.kernel.read_visual(&mut slots, &mut l, &mut r) };
    assert_eq!(summary.pool_count, 1);
    assert!(peak(&l) > 0.0);
    let clock = rig.kernel.clock();
    assert_eq!(clock.tempo, 97.5);
    assert!(!clock.from_host);
}

// ---- Presets -------------------------------------------------------------------------------------

#[test]
fn presets_are_named_in_range_and_leave_the_clock_alone() {
    assert_eq!(PRESETS.len(), 7);
    assert_eq!(PRESETS[0].name.to_str().unwrap(), "Default");
    for (i, spec) in PARAM_SPECS.iter().enumerate() {
        assert_eq!(PRESETS[0].values[i], spec.def);
    }
    for preset in &PRESETS {
        assert!(!preset.name.is_empty() && preset.name.to_bytes().len() <= 16);
        for (i, spec) in PARAM_SPECS.iter().enumerate() {
            let v = preset.values[i];
            assert!(v >= spec.min && v <= spec.max, "{:?} {:?} = {v}", preset.name, spec.identifier);
            if matches!(spec.unit, Unit::Indexed | Unit::Boolean) {
                assert_eq!(v, v.round());
            }
            if preset_leaves_alone(i as u64) {
                assert_eq!(v, spec.def);
            }
        }
    }
}

#[test]
fn presets_all_sound_stable() {
    let mut phrase = Buffer::new();
    for i in 0..6 {
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
    for preset in &PRESETS {
        let mut rig = KernelRig::new(SR);
        for (i, v) in preset.values.iter().enumerate() {
            rig.kernel.set_parameter(i as u64, *v);
        }
        rig.run(&phrase);
        rig.run(&silence(3.0, SR));
        rig.run(&phrase);
        assert!(all_finite(&rig.out_l), "{:?}", preset.name);
        assert!(peak(&rig.out_l) < 4.0, "{:?}", preset.name);
        assert!(rms(&rig.out_l, 0, rig.out_l.len()) > 1e-3, "{:?} is silent", preset.name);
    }
}

// ---- C ABI ---------------------------------------------------------------------------------------

#[test]
fn ffi_tolerates_null_pointers_and_bad_indexes() {
    unsafe {
        let null = std::ptr::null();
        sr_kernel_set_parameter(null, 0, 1.0);
        assert_eq!(sr_kernel_get_parameter(null, 0), 0.0);
        sr_kernel_prepare(null, 48000.0);
        sr_kernel_process(null, std::ptr::null(), null as _, null as _, std::ptr::null_mut(), std::ptr::null_mut(), 64);
        assert_eq!(
            sr_kernel_read_visual(
                null,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut()
            ),
            0
        );
        sr_kernel_free(std::ptr::null_mut());

        let k = sr_kernel_new();
        sr_kernel_prepare(k, f64::NAN); // ignored
        sr_kernel_prepare(k, 48000.0);
        let mut buf = [0.1f32; 64];
        let p = buf.as_mut_ptr();
        sr_kernel_process(k, std::ptr::null(), p, p, p, p.add(32), 0);
        sr_kernel_process(k, std::ptr::null(), p, p, p, p.add(32), -5);
        sr_kernel_process(k, std::ptr::null(), p, p, p, p.add(32), 32);
        assert_eq!(
            sr_kernel_read_visual(
                k,
                std::ptr::null_mut(),
                99,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                99,
                std::ptr::null_mut()
            ),
            0
        );
        sr_kernel_free(k);
    }
    assert!(sr_param_spec(999).identifier.is_null());
    assert!(sr_param_value_name(Param::Grid as u64, 8).is_null());
    assert!(sr_param_value_name(Param::Grid as u64, -1).is_null());
    assert!(sr_preset_name(-1).is_null() && sr_preset_name(7).is_null());
    assert_eq!(sr_preset_value(0, 999), 0.0);
}
