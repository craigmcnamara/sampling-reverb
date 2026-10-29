//! Stereo 8-line feedback delay network with input diffusion, modulation, damping, shimmer and sub.

use crate::util::{clampf, max, min, safe_limit, Allpass, DelayLine, Svf, PI, TWO_PI};

/// Two-grain crossfading delay-line pitch shifter.
pub struct PitchShifter {
    pub d: DelayLine,
    window: f32,
    pub phase: f32,
    ratio: f32,
}

impl Default for PitchShifter {
    fn default() -> Self {
        PitchShifter { d: DelayLine::default(), window: 1.0, phase: 0.0, ratio: 2.0 }
    }
}

impl PitchShifter {
    pub fn prepare(&mut self, sample_rate: f32) {
        self.window = 0.06 * sample_rate;
        self.d.allocate(self.window as u32 + 8);
        self.phase = 0.0;
    }

    pub fn set_semitones(&mut self, semis: f32) {
        self.ratio = 2f32.powf(semis / 12.0);
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        self.d.write(x);
        self.phase += (1.0 - self.ratio) / self.window;
        self.phase -= self.phase.floor();
        let mut p2 = self.phase + 0.5;
        if p2 >= 1.0 {
            p2 -= 1.0;
        }
        let w1 = (PI * self.phase).sin();
        let w2 = (PI * p2).sin();
        self.d.read(self.phase * self.window) * (w1 * w1) + self.d.read(p2 * self.window) * (w2 * w2)
    }
}

#[derive(Clone, Copy)]
pub struct ReverbParams {
    pub decay_sec: f32,
    pub size: f32,       // 0…1
    pub tone: f32,       // 0…1
    pub texture: f32,    // 0…1
    pub modulation: f32, // 0…1
    pub shimmer: f32,    // 0…1
    pub shimmer_semitones: f32,
    pub sub: f32, // 0…1
    pub freeze: bool,
}

impl Default for ReverbParams {
    fn default() -> Self {
        ReverbParams {
            decay_sec: 4.0,
            size: 0.6,
            tone: 0.6,
            texture: 0.5,
            modulation: 0.3,
            shimmer: 0.0,
            shimmer_semitones: 12.0,
            sub: 0.0,
            freeze: false,
        }
    }
}

const N: usize = 8;
/// Shimmer uses lines 0–3, Sub uses lines 4–7.
const SHIMMER_LINES: usize = N / 2;
/// Crossfade angle at 100%. Shimmer moves energy up into the range Tone damps fastest, so it gets
/// a gentler maximum (sin = 0.71) than Sub (sin = 0.92) to keep the reverb's body.
const MAX_SHIMMER_CROSSFADE: f32 = 0.5 * PI / 2.0;
const MAX_SUB_CROSSFADE: f32 = 0.75 * PI / 2.0;
/// Undertone series: f/2, f/2, f/3 (an octave and a fifth down) and f/4 (two octaves down).
const UNDERTONE_SEMITONES: [f32; SHIMMER_LINES] = [-12.0, -12.0, -19.0196, -24.0];
const MIN_SCALE: f32 = 0.25;
const MAX_SCALE: f32 = 2.0;

pub struct FdnReverb {
    sr: f32,
    p: ReverbParams,
    lines: [DelayLine; N],
    base_len: [f32; N],
    cur_len: [f32; N],
    target_len: [f32; N],
    gain: [f32; N],
    lp: [f32; N],
    lfo_phase: [f32; N],
    lfo_inc: [f32; N],
    lp_coef: f32,
    len_coef: f32,
    mod_depth: f32,
    diff_l: [Allpass; 4],
    diff_r: [Allpass; 4],
    shifters: [PitchShifter; SHIMMER_LINES],
    sub_shifters: [PitchShifter; SHIMMER_LINES],
    sub_hp: [Svf; SHIMMER_LINES],
    shimmer_hp: [f32; SHIMMER_LINES],
    shimmer_hp_coef: f32,
}

impl Default for FdnReverb {
    fn default() -> Self {
        FdnReverb {
            sr: 48000.0,
            p: ReverbParams::default(),
            lines: Default::default(),
            base_len: [0.0; N],
            cur_len: [0.0; N],
            target_len: [0.0; N],
            gain: [0.0; N],
            lp: [0.0; N],
            lfo_phase: [0.0; N],
            lfo_inc: [0.0; N],
            lp_coef: 0.5,
            len_coef: 0.001,
            mod_depth: 0.0,
            diff_l: Default::default(),
            diff_r: Default::default(),
            shifters: Default::default(),
            sub_shifters: Default::default(),
            sub_hp: [Svf::default(); SHIMMER_LINES],
            shimmer_hp: [0.0; SHIMMER_LINES],
            shimmer_hp_coef: 0.0,
        }
    }
}

impl FdnReverb {
    /// Allocates; not real-time safe.
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sr = sample_rate;
        const BASE_MS: [f32; N] = [29.7, 37.1, 41.1, 43.7, 53.3, 59.9, 67.7, 79.3];
        let max_mod = 0.0035 * self.sr;
        for j in 0..N {
            self.base_len[j] = BASE_MS[j] * 0.001 * self.sr;
            self.lines[j].allocate((self.base_len[j] * MAX_SCALE + max_mod) as u32 + 16);
            self.cur_len[j] = self.base_len[j];
            self.target_len[j] = self.base_len[j];
            self.lp[j] = 0.0;
            self.lfo_phase[j] = TWO_PI * j as f32 / N as f32;
            self.lfo_inc[j] = TWO_PI * (0.1 + 0.8 * j as f32 / (N - 1) as f32) / self.sr;
        }
        const DIFF_MS_L: [f32; 4] = [4.77, 3.59, 12.73, 9.31];
        const DIFF_MS_R: [f32; 4] = [5.11, 3.83, 11.87, 9.93];
        for k in 0..4 {
            self.diff_l[k].allocate((DIFF_MS_L[k] * 0.001 * self.sr) as u32);
            self.diff_r[k].allocate((DIFF_MS_R[k] * 0.001 * self.sr) as u32);
        }
        for k in 0..SHIMMER_LINES {
            self.shifters[k].prepare(self.sr);
            self.shifters[k].phase = 0.25 * k as f32; // stagger grain boundaries across lines
            self.sub_shifters[k].prepare(self.sr);
            self.sub_shifters[k].phase = 0.125 + 0.25 * k as f32;
            self.sub_shifters[k].set_semitones(UNDERTONE_SEMITONES[k]);
            self.sub_hp[k].set(55.0, 0.707, self.sr);
            self.sub_hp[k].reset();
            self.shimmer_hp[k] = 0.0;
        }
        self.shimmer_hp_coef = 1.0 - (-TWO_PI * 80.0 / self.sr).exp();
        self.len_coef = 1.0 - (-1.0 / (0.15 * self.sr)).exp();
    }

    pub fn set_params(&mut self, p: &ReverbParams) {
        self.p = *p;
        let scale = MIN_SCALE * (MAX_SCALE / MIN_SCALE).powf(clampf(p.size, 0.0, 1.0));
        let rt60 = max(p.decay_sec, 0.05);
        for j in 0..N {
            self.target_len[j] = self.base_len[j] * scale;
            self.gain[j] = if p.freeze { 1.0 } else { 10f32.powf(-3.0 * self.target_len[j] / (rt60 * self.sr)) };
        }
        let cutoff = 1500.0 * 12f32.powf(clampf(p.tone, 0.0, 1.0));
        self.lp_coef = 1.0 - (-TWO_PI * min(cutoff, 0.45 * self.sr) / self.sr).exp();
        let g = 0.75 * clampf(p.texture, 0.0, 1.0);
        for k in 0..4 {
            self.diff_l[k].g = g;
            self.diff_r[k].g = g;
        }
        self.mod_depth = clampf(p.modulation, 0.0, 1.0) * 0.003 * self.sr;
        for sh in &mut self.shifters {
            sh.set_semitones(p.shimmer_semitones);
        }
    }

    pub fn process(&mut self, in_l: &[f32], in_r: &[f32], out_l: &mut [f32], out_r: &mut [f32]) {
        // Shimmer and Sub live inside the feedback loop: on half the lines each, part of the
        // recirculating signal is swapped for its pitch-shifted copy. The swap is an equal-power
        // crossfade (cos/sin): the shifted copy is essentially uncorrelated with the original, so this
        // keeps the energy per pass, and the layers add richness instead of draining the reverb's
        // body. A linear crossfade lost up to half the energy every pass.
        let shimmer_angle = clampf(self.p.shimmer, 0.0, 1.0) * MAX_SHIMMER_CROSSFADE;
        let sub_angle = clampf(self.p.sub, 0.0, 1.0) * MAX_SUB_CROSSFADE;
        let (shimmer_dry, shimmer_wet) = (shimmer_angle.cos(), shimmer_angle.sin());
        let (sub_dry, sub_wet) = (sub_angle.cos(), sub_angle.sin());
        let freeze = self.p.freeze;
        let n = out_l.len();
        for i in 0..n {
            let mut xl = if freeze { 0.0 } else { in_l[i] };
            let mut xr = if freeze { 0.0 } else { in_r[i] };
            for k in 0..4 {
                xl = self.diff_l[k].process(xl);
                xr = self.diff_r[k].process(xr);
            }

            let mut o = [0f32; N];
            let mut s = [0f32; N];
            for j in 0..N {
                self.cur_len[j] += (self.target_len[j] - self.cur_len[j]) * self.len_coef;
                self.lfo_phase[j] += self.lfo_inc[j];
                if self.lfo_phase[j] > TWO_PI {
                    self.lfo_phase[j] -= TWO_PI;
                }
                o[j] = self.lines[j].read(self.cur_len[j] + self.mod_depth * self.lfo_phase[j].sin());
                self.lp[j] += self.lp_coef * (o[j] - self.lp[j]);
                s[j] = if freeze { o[j] } else { self.lp[j] };
            }
            hadamard8(&mut s);
            // Both shifted paths are high-passed: a grain shifter leaves very low frequencies (below
            // its grain rate) almost unshifted, so they'd stay correlated with the original and grow
            // on every pass under an equal-power crossfade.
            if shimmer_angle > 0.0 {
                for k in 0..SHIMMER_LINES {
                    let up = self.shifters[k].process(s[k]);
                    self.shimmer_hp[k] += self.shimmer_hp_coef * (up - self.shimmer_hp[k]);
                    s[k] = shimmer_dry * s[k] + shimmer_wet * (up - self.shimmer_hp[k]);
                }
            }
            // Sub follows the undertone series on the other half of the lines and cascades on every
            // pass. A 2-pole 55 Hz high-pass keeps the undertones of low notes (which land at
            // 20–40 Hz) from turning into rumble; the warmth stays in the 60–250 Hz range.
            if sub_angle > 0.0 {
                for k in 0..SHIMMER_LINES {
                    let shifted = self.sub_shifters[k].process(s[SHIMMER_LINES + k]);
                    let (_, hp) = self.sub_hp[k].process(shifted);
                    s[SHIMMER_LINES + k] = sub_dry * s[SHIMMER_LINES + k] + sub_wet * hp;
                }
            }
            for j in 0..N {
                let input = if j & 1 != 0 { xr } else { xl };
                let sign = if j & 2 != 0 { -1.0 } else { 1.0 };
                self.lines[j].write(safe_limit(sign * input * 0.5 + self.gain[j] * s[j]));
            }
            out_l[i] = (o[0] - o[2] + o[4] - o[6]) * 0.5;
            out_r[i] = (o[1] - o[3] + o[5] - o[7]) * 0.5;
        }
    }

    pub fn clear(&mut self) {
        for l in &mut self.lines {
            l.clear();
        }
        for a in self.diff_l.iter_mut().chain(self.diff_r.iter_mut()) {
            a.d.clear();
        }
        for sh in self.shifters.iter_mut().chain(self.sub_shifters.iter_mut()) {
            sh.d.clear();
        }
        for f in &mut self.sub_hp {
            f.reset();
        }
        self.shimmer_hp = [0.0; SHIMMER_LINES];
        self.lp = [0.0; N];
    }
}

#[inline]
fn hadamard8(x: &mut [f32; N]) {
    let mut h = 1;
    while h < N {
        let mut i = 0;
        while i < N {
            for j in i..i + h {
                let (a, b) = (x[j], x[j + h]);
                x[j] = a + b;
                x[j + h] = a - b;
            }
            i += h * 2;
        }
        h <<= 1;
    }
    for v in x.iter_mut() {
        *v *= 0.35355339; // 1/sqrt(8)
    }
}
