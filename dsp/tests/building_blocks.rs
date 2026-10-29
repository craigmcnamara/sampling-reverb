//! DSP building blocks: utilities, delay lines, filters, half floats and the triple buffer.

mod common;

use common::*;
use sampling_reverb_dsp::triple_buffer::TripleBuffer;
use sampling_reverb_dsp::util::*;

const SR: f32 = 48000.0;

#[test]
fn next_pow2_rounds_up() {
    assert_eq!(next_pow2(1), 1);
    assert_eq!(next_pow2(3), 4);
    assert_eq!(next_pow2(4), 4);
    assert_eq!(next_pow2(1025), 2048);
}

#[test]
fn safe_limit_is_transparent_below_unity_and_bounded() {
    assert_eq!(safe_limit(0.5), 0.5);
    assert_eq!(safe_limit(-1.0), -1.0);
    assert!(safe_limit(1.5) > 1.0);
    assert!(safe_limit(1000.0) <= 2.0);
    assert!(safe_limit(-1000.0) >= -2.0);
}

#[test]
fn delay_line_reads_integer_and_fractional_delays() {
    let mut d = DelayLine::default();
    d.allocate(16);
    for i in 1..=8 {
        d.write(i as f32);
    }
    assert_eq!(d.read_int(0), 8.0);
    assert_eq!(d.read_int(3), 5.0);
    assert_near!(d.read(0.0), 8.0, 1e-6);
    assert_near!(d.read(1.5), 6.5, 1e-6);
    // Delays beyond the buffer wrap around instead of reading out of bounds.
    let _ = d.read(1e6);
    let _ = d.read_int(u32::MAX);
}

#[test]
fn allpass_preserves_impulse_energy() {
    let mut ap = Allpass::default();
    ap.allocate(37);
    ap.g = 0.6;
    let energy: f64 = (0..20000).map(|i| ap.process(if i == 0 { 1.0 } else { 0.0 }) as f64).map(|y| y * y).sum();
    assert_near!(energy, 1.0, 1e-3);
}

#[test]
fn svf_low_and_high_pass_split_the_spectrum() {
    let measure = |hz: f32, high: bool| {
        let mut f = Svf::default();
        f.set(1000.0, 0.707, SR);
        let x = sine(hz, 0.5, 1.0, SR);
        let y: Buffer = x
            .iter()
            .map(|&v| {
                let (lp, hp) = f.process(v);
                if high {
                    hp
                } else {
                    lp
                }
            })
            .collect();
        rms(&y, y.len() / 2, y.len()) / rms(&x, x.len() / 2, x.len())
    };
    assert!(measure(100.0, false) > 0.95);
    assert!(measure(10000.0, false) < 0.02);
    assert!(measure(10000.0, true) > 0.95);
    assert!(measure(100.0, true) < 0.02);
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
    assert_eq!(F16::from_f32(1.0 + 1.0 / 2048.0).to_f32(), 1.0); // tie → even (down)
    assert_eq!(F16::from_f32(1.0 + 3.0 / 2048.0).to_f32(), 1.0 + 2.0 / 1024.0); // tie → even (up)
    assert_eq!(F16::from_f32(65520.0).to_f32(), f32::INFINITY);
    assert_eq!(F16::from_f32(65519.0).to_f32(), 65504.0);
    assert_eq!(F16::from_f32(-0.0).0, 0x8000);
    assert_eq!(F16::from_f32(5.9604645e-8).to_f32(), 5.9604645e-8); // smallest subnormal
    assert_eq!(F16::from_f32(2.9802322e-8).to_f32(), 0.0); // half of it: tie → even (zero)
}

#[test]
fn triple_buffer_reader_sees_latest_publish_only() {
    let tb = TripleBuffer::<i32>::default();
    unsafe {
        assert_eq!(*tb.read(), 0);
        *tb.back_buffer() = 1;
        assert_eq!(*tb.read(), 0); // not yet published
        tb.publish();
        *tb.back_buffer() = 2;
        tb.publish();
        *tb.back_buffer() = 3;
        tb.publish();
        assert_eq!(*tb.read(), 3);
        assert_eq!(*tb.read(), 3); // stable without new publishes
    }
}

#[test]
fn triple_buffer_never_tears_under_concurrency() {
    let tb = std::sync::Arc::new(TripleBuffer::<(i64, i64)>::default());
    let producer = {
        let tb = tb.clone();
        std::thread::spawn(move || {
            for i in 1..=200_000i64 {
                unsafe {
                    *tb.back_buffer() = (i, -i);
                    tb.publish();
                }
            }
        })
    };
    let (mut torn, mut regressions, mut last) = (0, 0, 0);
    while !producer.is_finished() {
        let (a, b) = unsafe { *tb.read() };
        torn += (a != -b) as i32;
        regressions += (a < last) as i32;
        last = a;
    }
    producer.join().unwrap();
    assert_eq!(torn, 0);
    assert_eq!(regressions, 0);
    assert_eq!(unsafe { tb.read().0 }, 200_000);
}
