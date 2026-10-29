//! Test signal generators and measurements (a port of Tests/DSP/Signals.hpp).
#![allow(dead_code)]
// The generators copy the C++ test signals, including their π literal.
#![allow(clippy::approx_constant, clippy::excessive_precision)]

use sampling_reverb_dsp::util::Svf;

pub type Buffer = Vec<f32>;

pub fn frames(seconds: f32, sr: f32) -> usize {
    (seconds * sr) as usize
}

pub fn silence(seconds: f32, sr: f32) -> Buffer {
    vec![0.0; frames(seconds, sr)]
}

/// Sine burst with a 5 ms attack and an exponential release, like a plucked note.
pub fn tone_burst(hz: f32, seconds: f32, amp: f32, sr: f32) -> Buffer {
    let n = frames(seconds, sr);
    let attack = 0.005 * sr;
    (0..n)
        .map(|i| {
            let t = i as f32;
            let env = (t / attack).min(1.0) * (-3.0 * t / n as f32).exp();
            amp * env * (2.0 * 3.14159265 * hz * t / sr).sin()
        })
        .collect()
}

pub fn noise_burst(seconds: f32, amp: f32, sr: f32, mut seed: u32) -> Buffer {
    let n = frames(seconds, sr);
    let attack = 0.002 * sr;
    (0..n)
        .map(|i| {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let x = (seed >> 8) as f32 * (2.0 / 16777216.0) - 1.0;
            let t = i as f32;
            amp * x * (t / attack).min(1.0) * (-4.0 * t / n as f32).exp()
        })
        .collect()
}

pub fn sine(hz: f32, seconds: f32, amp: f32, sr: f32) -> Buffer {
    (0..frames(seconds, sr)).map(|i| amp * (2.0 * 3.14159265 * hz * i as f32 / sr).sin()).collect()
}

/// Karplus-Strong plucked string: a stand-in for a guitar note.
pub fn pluck(hz: f32, seconds: f32, amp: f32, sr: f32, mut seed: u32) -> Buffer {
    let period = ((sr / hz) as usize).max(2);
    let mut line: Vec<f32> = (0..period)
        .map(|_| {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 * (2.0 / 16777216.0) - 1.0
        })
        .collect();
    let mut idx = 0;
    (0..frames(seconds, sr))
        .map(|_| {
            let next = (idx + 1) % period;
            let out = line[idx];
            line[idx] = 0.996 * 0.5 * (line[idx] + line[next]);
            idx = next;
            amp * out
        })
        .collect()
}

/// A strummed open E-minor-ish chord, 12 ms between strings.
pub fn strum(seconds: f32, amp: f32, sr: f32) -> Buffer {
    let notes = [82.41, 123.47, 164.81, 196.0, 246.94, 329.63];
    let mut b = vec![0.0; frames(seconds, sr)];
    for (k, hz) in notes.iter().enumerate() {
        let s = pluck(*hz, seconds, amp / 3.0, sr, 11 + k as u32);
        let offset = frames(0.012 * k as f32, sr);
        for i in 0..b.len().saturating_sub(offset) {
            b[i + offset] += s[i];
        }
    }
    b
}

pub fn append(dst: &mut Buffer, src: &[f32]) {
    dst.extend_from_slice(src);
}

pub fn rms(b: &[f32], from: usize, to: usize) -> f32 {
    let to = to.min(b.len());
    if from >= to {
        return 0.0;
    }
    let s: f64 = b[from..to].iter().map(|&x| x as f64 * x as f64).sum();
    (s / (to - from) as f64).sqrt() as f32
}

pub fn peak(b: &[f32]) -> f32 {
    b.iter().fold(0.0, |p, x| p.max(x.abs()))
}

pub fn all_finite(b: &[f32]) -> bool {
    b.iter().all(|x| x.is_finite())
}

pub fn db(g: f32) -> f32 {
    20.0 * g.max(1e-12).log10()
}

/// Frequency estimate from rising zero crossings.
pub fn zero_crossing_hz(b: &[f32], from: usize, to: usize, sr: f32) -> f32 {
    let (mut crossings, mut first, mut last) = (0, 0, 0);
    for i in from + 1..to.min(b.len()) {
        if b[i - 1] < 0.0 && b[i] >= 0.0 {
            if crossings == 0 {
                first = i;
            }
            last = i;
            crossings += 1;
        }
    }
    if crossings > 1 {
        (crossings - 1) as f32 * sr / (last - first) as f32
    } else {
        0.0
    }
}

/// Level of one frequency component over [from, to) (Goertzel, Hann window), as an amplitude.
pub fn tone_level(b: &[f32], from: usize, to: usize, hz: f32, sr: f32) -> f32 {
    let to = to.min(b.len());
    if to <= from {
        return 0.0;
    }
    let n = to - from;
    let w = 2.0 * std::f64::consts::PI * hz as f64 / sr as f64;
    let coeff = 2.0 * w.cos();
    let (mut s1, mut s2, mut norm) = (0.0f64, 0.0f64, 0.0f64);
    for i in 0..n {
        let hann = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos();
        let s0 = b[from + i] as f64 * hann + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
        norm += hann;
    }
    let power = s1 * s1 + s2 * s2 - coeff * s1 * s2;
    (2.0 * power.max(0.0).sqrt() / norm) as f32
}

/// Energy (dB) in the band [lo, hi] Hz from `from` on: two 2-pole high-passes, two 2-pole low-passes.
pub fn band_energy_db(x: &[f32], from: usize, lo: f32, hi: f32, sr: f32) -> f32 {
    let mut f = [Svf::default(); 4];
    f[0].set(lo, 0.707, sr);
    f[1].set(lo, 0.707, sr);
    f[2].set(hi, 0.707, sr);
    f[3].set(hi, 0.707, sr);
    let mut e = 0.0f64;
    for (i, &v) in x.iter().enumerate() {
        let (_, hp) = f[0].process(v);
        let (_, hp) = f[1].process(hp);
        let (lp, _) = f[2].process(hp);
        let (lp, _) = f[3].process(lp);
        if i >= from {
            e += lp as f64 * lp as f64;
        }
    }
    (10.0 * (e + 1e-20).log10()) as f32
}
