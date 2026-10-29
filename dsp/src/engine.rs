//! Onset detection, feature analysis, the sample pool (with retention) and the voice bank.

use crate::util::{clampf, db_to_gain, gain_to_db, lerpf, lround, max, min, next_pow2, Rng, F16, PI, TWO_PI};
use crate::visual::SRSlotVisual;

#[derive(Clone, Copy, Debug)]
pub struct Features {
    pub bright: f32,  // 0…1
    pub clarity: f32, // 0…1
    pub midi: f32,    // < 0 when unpitched
    pub loud_db: f32,
    pub x: f32,
    pub y: f32,
}

impl Default for Features {
    fn default() -> Self {
        Features { bright: 0.0, clarity: 0.0, midi: -1.0, loud_db: -120.0, x: 0.5, y: 0.5 }
    }
}

#[derive(Clone, Copy)]
pub struct EngineParams {
    pub retention: f32, // 0…1, ≥ 0.995 = keep forever
    pub sensitivity_db: f32,
    pub match_tolerance: f32, // 0…1
    pub harmony: f32,         // 0 = similar, 1 = complementary
    pub length_sec: f32,
    pub attack_sec: f32, // fade-in of each replayed sample
    pub density: i32,
    pub focus_x: f32,
    pub focus_y: f32,
    pub focus_amount: f32,
    pub freeze: bool,
    pub quantize: bool,  // hold matched triggers until the next grid point
    pub grid_beats: f64, // grid spacing in quarter-note beats
}

impl Default for EngineParams {
    fn default() -> Self {
        EngineParams {
            retention: 0.5,
            sensitivity_db: -40.0,
            match_tolerance: 0.4,
            harmony: 0.3,
            length_sec: 0.8,
            attack_sec: 0.1,
            density: 4,
            focus_x: 0.5,
            focus_y: 0.5,
            focus_amount: 0.0,
            freeze: false,
            quantize: false,
            grid_beats: 0.5,
        }
    }
}

/// The musical position at the start of the current chunk, set by the kernel.
#[derive(Clone, Copy, Default)]
pub struct ClockState {
    pub beat: f64,            // quarter-note beats
    pub beats_per_frame: f64, // 0 = no clock
    pub bar_origin: f64,      // beat of a bar line; the grid is laid out from here
}

pub const SLOTS: usize = crate::visual::SR_MAX_SLOTS;
const VOICES: usize = 16; // room for voices still fading out after a Density steal
const RECORDERS: usize = 3; // captures that can record at the same time
const PENDING: usize = 8; // quantized triggers waiting for their grid point
pub const MAX_LENGTH_SEC: f32 = 20.0;

#[derive(Clone, Copy, Default)]
struct Slot {
    active: bool,
    recording: bool, // reserved by a Recorder; not playable until it finishes
    generation: u32,
    trigger_count: u32,
    offset: usize, // start of this slot's region in the pool
    length: u32,
    peak: f32,
    f: Features,
    life: f32,
    last_trigger: u64,
}

#[derive(Clone, Copy)]
struct Voice {
    active: bool,
    releasing: bool,
    slot: usize,
    pos: u32,
    env: f32,
    env_inc: f32,
    gl: f32,
    gr: f32,
    meter: f32,
    start: u64,
}

impl Default for Voice {
    fn default() -> Self {
        Voice {
            active: false,
            releasing: false,
            slot: 0,
            pos: 0,
            env: 0.0,
            env_inc: 0.0,
            gl: 1.0,
            gr: 1.0,
            meter: 0.0,
            start: 0,
        }
    }
}

/// Writes a capture into its slot a sample at a time, so long captures never need a big copy on the
/// audio thread.
#[derive(Clone, Copy, Default)]
struct Recorder {
    active: bool,
    slot: usize,
    start: u64, // absolute frames, [start, end)
    end: u64,
    written: u32,
    peak: f32,
    f: Features,
}

#[derive(Clone, Copy, Default)]
struct Pending {
    active: bool,
    slot: usize,
    due: u64, // absolute frame
}

pub struct SampleEngine {
    sr: f32,
    p: EngineParams,

    hist_l: Vec<f32>,
    hist_r: Vec<f32>,
    hist_mask: u32,
    frame: u64,

    fast: f32,
    slow: f32,
    fast_att: f32,
    fast_rel: f32,
    slow_coef: f32,
    holdoff: u32,
    analyzing: bool,
    onset_frame: u64,
    window: u32,
    preroll: u32,
    max_len: u32,

    recorders: [Recorder; RECORDERS],
    pending: [Pending; PENDING],
    pending_count: usize,
    clock: ClockState,
    clock_frame: u64,
    // Pool samples are stored as half floats: ~66 dB of precision relative to the signal at any
    // level, at half the memory (24 slots x 20 s stereo is ~92 MB at 48 kHz instead of ~184 MB).
    pool_l: Vec<F16>,
    pool_r: Vec<F16>,
    slots: [Slot; SLOTS],
    voices: [Voice; VOICES],
    meter_decay: f32,
    bus_gain: f32,
    bus_coef: f32,
    release_inc: f32,
    rng: Rng,

    mono: Vec<f32>,
    dec: Vec<f32>,
    acf: Vec<f32>,
    last_features: Features,
    total_triggers: u64,
    total_captures: u64,
}

impl Default for SampleEngine {
    fn default() -> Self {
        SampleEngine {
            sr: 48000.0,
            p: EngineParams::default(),
            hist_l: Vec::new(),
            hist_r: Vec::new(),
            hist_mask: 0,
            frame: 0,
            fast: 0.0,
            slow: 0.0,
            fast_att: 0.0,
            fast_rel: 0.0,
            slow_coef: 0.0,
            holdoff: 0,
            analyzing: false,
            onset_frame: 0,
            window: 2048,
            preroll: 480,
            max_len: 0,
            recorders: Default::default(),
            pending: Default::default(),
            pending_count: 0,
            clock: ClockState::default(),
            clock_frame: 0,
            pool_l: Vec::new(),
            pool_r: Vec::new(),
            slots: [Slot::default(); SLOTS],
            voices: [Voice::default(); VOICES],
            meter_decay: 0.0,
            bus_gain: 1.0,
            bus_coef: 0.0,
            release_inc: 0.0,
            rng: Rng::default(),
            mono: Vec::new(),
            dec: Vec::new(),
            acf: Vec::new(),
            last_features: Features::default(),
            total_triggers: 0,
            total_captures: 0,
        }
    }
}

impl SampleEngine {
    /// Allocates; not real-time safe.
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sr = sample_rate;
        self.preroll = (0.01 * self.sr) as u32;
        self.window = if self.sr > 64000.0 { 4096 } else { 2048 };
        self.max_len = (MAX_LENGTH_SEC * self.sr) as u32 + self.preroll;
        // The history only needs the pre-roll and analysis window: captures record straight into
        // their slot as audio arrives.
        let hist_size = next_pow2(self.preroll + self.window + 16384);
        self.hist_l = vec![0.0; hist_size as usize];
        self.hist_r = vec![0.0; hist_size as usize];
        self.hist_mask = hist_size - 1;
        let pool = self.max_len as usize * SLOTS;
        self.pool_l = vec![F16::default(); pool];
        self.pool_r = vec![F16::default(); pool];
        for (s, slot) in self.slots.iter_mut().enumerate() {
            *slot = Slot { offset: s * self.max_len as usize, ..Slot::default() };
        }
        self.voices = [Voice::default(); VOICES];
        self.recorders = Default::default();
        self.pending = Default::default();
        self.pending_count = 0;
        self.clock = ClockState::default();
        self.clock_frame = 0;
        self.mono = vec![0.0; self.window as usize];
        self.dec = vec![0.0; self.window as usize];
        self.acf = vec![0.0; self.window as usize];

        self.frame = 0;
        self.fast = 0.0;
        self.slow = 0.0;
        self.holdoff = 0;
        self.analyzing = false;
        self.fast_att = (-1.0 / (0.001 * self.sr)).exp();
        self.fast_rel = (-1.0 / (0.010 * self.sr)).exp();
        self.slow_coef = (-1.0 / (0.150 * self.sr)).exp();
        self.meter_decay = (-1.0 / (0.08 * self.sr)).exp();
        self.release_inc = 1.0 / (0.6 * self.sr); // a voice pushed out by Density fades, never chops
        self.bus_gain = 1.0;
        self.bus_coef = 1.0 - (-1.0 / (0.05 * self.sr)).exp();
    }

    pub fn set_params(&mut self, p: &EngineParams) {
        self.p = *p;
    }

    /// Call before each process(): the beat position at the first frame of the coming chunk.
    pub fn set_clock(&mut self, c: ClockState) {
        self.clock = c;
        self.clock_frame = self.frame;
    }

    /// Analyzes the input and renders triggered voices into out (which does not include the input).
    pub fn process(&mut self, in_l: &[f32], in_r: &[f32], out_l: &mut [f32], out_r: &mut [f32]) {
        let thresh = db_to_gain(self.p.sensitivity_db);
        for i in 0..out_l.len() {
            let (l, r) = (in_l[i], in_r[i]);
            let idx = (self.frame as u32 & self.hist_mask) as usize;
            self.hist_l[idx] = l;
            self.hist_r[idx] = r;

            let m = 0.5 * (l.abs() + r.abs());
            self.fast = m + (self.fast - m) * if m > self.fast { self.fast_att } else { self.fast_rel };
            self.slow = self.fast + (self.slow - self.fast) * self.slow_coef;
            self.frame += 1;

            for k in 0..RECORDERS {
                if !self.recorders[k].active {
                    continue;
                }
                self.record(k, l, r);
                let rec = &self.recorders[k];
                if rec.start + rec.written as u64 >= rec.end {
                    self.finish_recording(k);
                }
            }

            if self.holdoff > 0 {
                // When the hold-off ends, re-baseline the slow envelope so the tail of the same
                // attack can't count as a new onset; only a genuinely new attack can.
                self.holdoff -= 1;
                if self.holdoff == 0 {
                    self.slow = max(self.slow, self.fast);
                }
            } else if !self.analyzing && self.fast > thresh && self.fast > self.slow * 2.0 {
                self.analyzing = true;
                self.onset_frame = self.frame - 1;
                self.holdoff = (0.08 * self.sr) as u32;
            }
            if self.analyzing && self.frame >= self.onset_frame + self.window as u64 {
                self.analyzing = false;
                self.handle_onset();
            }
            if self.pending_count > 0 {
                self.fire_pending();
            }

            let (mut ol, mut or) = (0.0f32, 0.0f32);
            let mut sounding = 0.0f32;
            for v in &mut self.voices {
                if !v.active {
                    continue;
                }
                let s = &self.slots[v.slot];
                if v.pos >= s.length {
                    v.active = false;
                    continue;
                }
                v.env += v.env_inc;
                if v.env >= 1.0 {
                    v.env = 1.0;
                    v.env_inc = 0.0;
                } else if v.env <= 0.0 {
                    v.active = false;
                    continue;
                }
                let at = s.offset + v.pos as usize;
                let sl = self.pool_l[at].to_f32() * v.env;
                let sr = self.pool_r[at].to_f32() * v.env;
                v.pos += 1;
                sounding += v.env * v.env;
                ol += sl * v.gl;
                or += sr * v.gr;
                v.meter = max(0.5 * (sl.abs() + sr.abs()), v.meter * self.meter_decay);
            }
            // Overlapping replays are largely uncorrelated, so their energy adds: scale the bus by
            // 1/sqrt(voices sounding) so a dense pool doesn't swamp the live signal.
            let bus_target = 1.0 / max(sounding, 1.0).sqrt();
            self.bus_gain += (bus_target - self.bus_gain) * self.bus_coef;
            out_l[i] = ol * self.bus_gain;
            out_r[i] = or * self.bus_gain;
        }
    }

    /// Ages the pool. Call once per processed block.
    pub fn advance_life(&mut self, seconds: f32) {
        let ml = self.max_life();
        if self.p.freeze || ml.is_infinite() {
            return;
        }
        for s in 0..SLOTS {
            if !self.slots[s].active {
                continue;
            }
            self.slots[s].life -= seconds;
            if self.slots[s].life <= 0.0 {
                self.slots[s].life = 0.0;
                if !self.is_playing(s) {
                    self.slots[s].active = false;
                }
            }
        }
    }

    pub fn pool_count(&self) -> i32 {
        self.slots.iter().filter(|s| s.active).count() as i32
    }

    pub fn fill_visual(&self, out: &mut [SRSlotVisual; SLOTS]) {
        let ml = self.max_life();
        for (s, slot) in self.slots.iter().enumerate() {
            let v = &mut out[s];
            v.active = slot.active as i32;
            v.generation = slot.generation;
            v.trigger_count = slot.trigger_count;
            v.x = slot.f.x;
            v.y = slot.f.y;
            v.clarity = slot.f.clarity;
            v.life = if ml.is_infinite() { 1.0 } else { clampf(slot.life / ml, 0.0, 2.0) };
            let mut level = 0.0f32;
            for voice in &self.voices {
                if voice.active && voice.slot == s {
                    level = max(level, voice.meter / max(slot.peak, 1e-4));
                }
            }
            v.level = clampf(level, 0.0, 1.0);
        }
    }

    // Introspection (used by tests).
    pub fn total_triggers(&self) -> u64 {
        self.total_triggers
    }
    pub fn total_captures(&self) -> u64 {
        self.total_captures
    }
    pub fn last_features(&self) -> &Features {
        &self.last_features
    }
    pub fn active_voice_count(&self) -> i32 {
        self.voices.iter().filter(|v| v.active && !v.releasing).count() as i32
    }
    pub fn slot_length(&self, s: usize) -> u32 {
        if self.slots[s].active {
            self.slots[s].length
        } else {
            0
        }
    }

    fn max_life(&self) -> f32 {
        if self.p.retention >= 0.995 {
            return f32::INFINITY;
        }
        3.0 * 100f32.powf(clampf(self.p.retention, 0.0, 1.0))
    }

    fn is_playing(&self, s: usize) -> bool {
        self.voices.iter().any(|v| v.active && v.slot == s) || self.is_pending(s)
    }

    fn is_pending(&self, s: usize) -> bool {
        self.pending.iter().any(|t| t.active && t.slot == s)
    }

    // ---- Analysis -------------------------------------------------------------------------------

    fn analyze(&mut self, start: u64) -> Features {
        let mut f = Features::default();
        let w = self.window as usize;
        let hp_a = 1.0 - (-TWO_PI * 1500.0 / self.sr).exp();
        let (mut total, mut high) = (0.0f64, 0.0f64);
        let mut lp = 0.0f32;
        for k in 0..w {
            let idx = ((start + k as u64) as u32 & self.hist_mask) as usize;
            let x = 0.5 * (self.hist_l[idx] + self.hist_r[idx]);
            self.mono[k] = x;
            lp += hp_a * (x - lp);
            let h = x - lp;
            total += (x * x) as f64;
            high += (h * h) as f64;
        }
        f.loud_db = gain_to_db((total / w as f64).sqrt() as f32);
        let ratio = if total > 1e-12 { (high / total) as f32 } else { 0.0 };
        f.bright = clampf(ratio.sqrt() * 1.2, 0.0, 1.0);

        // Pitch: normalized autocorrelation on a ~12 kHz decimated copy.
        let d = (lround(self.sr / 12000.0) as i32).max(1);
        let n = w as i32 / d;
        let dsr = self.sr / d as f32;
        let mut mean = 0.0f32;
        for j in 0..n as usize {
            let mut s = 0.0f32;
            for q in 0..d as usize {
                s += self.mono[j * d as usize + q];
            }
            self.dec[j] = s / d as f32;
            mean += self.dec[j];
        }
        mean /= n as f32;
        for j in 0..n as usize {
            self.dec[j] -= mean;
        }

        let min_lag = ((dsr / 1200.0) as i32).max(2);
        let max_lag = ((dsr / 60.0) as i32 + 1).min(n - 64);
        let mut rmax = 0.0f32;
        for tau in min_lag..=max_lag {
            let (mut num, mut e1, mut e2) = (0.0f32, 0.0f32, 0.0f32);
            for i in 0..(n - tau) as usize {
                let (a, b) = (self.dec[i], self.dec[i + tau as usize]);
                num += a * b;
                e1 += a * a;
                e2 += b * b;
            }
            let r = num / (e1 * e2 + 1e-12).sqrt();
            self.acf[tau as usize] = r;
            rmax = max(rmax, r);
        }
        let mut best = -1i32;
        for tau in (min_lag + 1)..max_lag {
            let t = tau as usize;
            if self.acf[t] >= 0.9 * rmax && self.acf[t] >= self.acf[t - 1] && self.acf[t] >= self.acf[t + 1] {
                best = tau;
                break;
            }
        }
        f.clarity = clampf(rmax, 0.0, 1.0);
        if best > 0 && f.clarity >= 0.5 {
            let b = best as usize;
            let (a, bb, c) = (self.acf[b - 1], self.acf[b], self.acf[b + 1]);
            let denom = a - 2.0 * bb + c;
            let shift = if denom.abs() > 1e-9 { 0.5 * (a - c) / denom } else { 0.0 };
            let hz = dsr / (best as f32 + shift);
            f.midi = 69.0 + 12.0 * (hz / 440.0).log2();
        }

        f.x = f.bright;
        f.y = if f.midi >= 0.0 { (f.midi + 0.5) % 12.0 / 12.0 } else { clampf((f.loud_db + 60.0) / 60.0, 0.0, 1.0) };
        f
    }

    fn distance(&self, a: &Features, b: &Features) -> f32 {
        const SIMILAR: [f32; 12] = [0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
        const COMPLEMENT: [f32; 12] = [0.3, 1.0, 0.6, 0.15, 0.15, 0.05, 1.0, 0.05, 0.15, 0.15, 0.6, 1.0];
        let mut d = (a.bright - b.bright).abs() + 0.5 * (a.clarity - b.clarity).abs();
        let (pa, pb) = (a.midi >= 0.0, b.midi >= 0.0);
        if pa && pb {
            let iv = ((lround(a.midi - b.midi) % 12 + 12) % 12) as usize;
            d += 0.6 * lerpf(SIMILAR[iv], COMPLEMENT[iv], self.p.harmony);
        } else if pa != pb {
            d += 0.3;
        }
        let (dx, dy) = (b.x - self.p.focus_x, b.y - self.p.focus_y);
        d += self.p.focus_amount * ((dx * dx + dy * dy).sqrt() - 0.3);
        d
    }

    // ---- Pool -------------------------------------------------------------------------------------

    fn handle_onset(&mut self) {
        let f = self.analyze(self.onset_frame);
        self.last_features = f;
        let retrig_guard = (0.1 * self.sr) as u64;
        let mut best: Option<usize> = None;
        let mut best_d = 0.03 + 0.57 * self.p.match_tolerance;
        for s in 0..SLOTS {
            let slot = &self.slots[s];
            if !slot.active || self.frame.wrapping_sub(slot.last_trigger) < retrig_guard || self.is_pending(s) {
                continue;
            }
            let d = self.distance(&f, &slot.f);
            if d < best_d {
                best_d = d;
                best = Some(s);
            }
        }
        if let Some(s) = best {
            self.schedule_trigger(s);
        } else if !self.p.freeze && !self.is_being_recorded(&f, 0.03 + 0.57 * self.p.match_tolerance) {
            self.start_recording(f);
        }
    }

    /// A sound similar to one still being recorded is a repeat, not something new to capture.
    fn is_being_recorded(&self, f: &Features, threshold: f32) -> bool {
        self.recorders.iter().any(|r| r.active && self.distance(f, &r.f) < threshold)
    }

    fn choose_capture_slot(&self) -> Option<usize> {
        if let Some(s) = (0..SLOTS).find(|&s| !self.slots[s].active && !self.slots[s].recording) {
            return Some(s);
        }
        let mut target = None;
        let mut lowest = f32::INFINITY;
        for s in 0..SLOTS {
            if self.slots[s].recording || self.is_playing(s) {
                continue;
            }
            if self.slots[s].life < lowest {
                lowest = self.slots[s].life;
                target = Some(s);
            }
        }
        target
    }

    fn start_recording(&mut self, f: Features) {
        let Some(k) = self.recorders.iter().position(|r| !r.active) else { return };
        let Some(target) = self.choose_capture_slot() else { return };

        let slot = &mut self.slots[target];
        slot.active = false;
        slot.recording = true;
        let start = self.onset_frame.saturating_sub(self.preroll as u64);
        let len = (clampf(self.p.length_sec, 0.05, MAX_LENGTH_SEC) * self.sr) as u32;
        self.recorders[k] = Recorder {
            active: true,
            slot: target,
            start,
            end: (self.onset_frame + len as u64).min(start + self.max_len as u64),
            written: 0,
            peak: 0.0,
            f,
        };
        // Catch up on the pre-roll and analysis window, which are still in the history.
        let mut t = start;
        while t < self.frame && t < self.recorders[k].end {
            let idx = (t as u32 & self.hist_mask) as usize;
            let (l, r) = (self.hist_l[idx], self.hist_r[idx]);
            self.record(k, l, r);
            t += 1;
        }
        let rec = &self.recorders[k];
        if rec.start + rec.written as u64 >= rec.end {
            self.finish_recording(k);
        }
    }

    fn record(&mut self, k: usize, l: f32, r: f32) {
        let rec = &mut self.recorders[k];
        let offset = self.slots[rec.slot].offset;
        let i = rec.written;
        rec.written += 1;
        let fade_in = self.preroll.max(1);
        let g = if i < fade_in { i as f32 / fade_in as f32 } else { 1.0 };
        // The recorder's end is capped at max_len, so the write stays inside this slot's region.
        self.pool_l[offset + i as usize] = F16::from_f32(l * g);
        self.pool_r[offset + i as usize] = F16::from_f32(r * g);
        rec.peak = max(rec.peak, 0.5 * (l.abs() + r.abs()) * g);
    }

    fn finish_recording(&mut self, k: usize) {
        let rec = self.recorders[k];
        let offset = self.slots[rec.slot].offset;
        // The capture may end mid-note, so taper the last quarter (30 ms to 2 s) with a raised cosine
        // rather than chopping it.
        let len = rec.written;
        let fade_sec = clampf(0.25 * len as f32 / self.sr, 0.03, 2.0);
        let fade_out = ((fade_sec * self.sr) as u32).min(len / 2).max(1);
        let mut i = len.saturating_sub(fade_out);
        while i < len {
            let x = (len - i) as f32 / fade_out as f32; // 1 → 0 across the fade
            let g = 0.5 - 0.5 * (PI * x).cos();
            let at = offset + i as usize;
            self.pool_l[at] = F16::from_f32(self.pool_l[at].to_f32() * g);
            self.pool_r[at] = F16::from_f32(self.pool_r[at].to_f32() * g);
            i += 1;
        }
        let ml = self.max_life();
        let frame = self.frame;
        let slot = &mut self.slots[rec.slot];
        slot.recording = false;
        slot.active = true;
        slot.generation = slot.generation.wrapping_add(1);
        slot.length = len;
        slot.peak = rec.peak;
        slot.f = rec.f;
        slot.life = if ml.is_infinite() { 1.0 } else { ml };
        slot.last_trigger = frame;
        self.recorders[k].active = false;
        self.total_captures += 1;
    }

    // ---- Quantize -------------------------------------------------------------------------------

    /// Frames from now to the next grid point (0 when quantize is off or there is no clock).
    fn frames_to_grid(&self) -> u64 {
        if !self.p.quantize || self.clock.beats_per_frame <= 0.0 || self.p.grid_beats <= 0.0 {
            return 0;
        }
        // frame has already advanced past the current sample.
        let beat = self.clock.beat + (self.frame - 1 - self.clock_frame) as f64 * self.clock.beats_per_frame;
        let rel = (beat - self.clock.bar_origin) / self.p.grid_beats;
        let wait = ((rel - 1e-9).ceil() - rel) * self.p.grid_beats / self.clock.beats_per_frame;
        wait.round() as i64 as u64
    }

    fn schedule_trigger(&mut self, s: usize) {
        let wait = self.frames_to_grid();
        if wait == 0 {
            return self.trigger(s);
        }
        if let Some(t) = self.pending.iter_mut().find(|t| !t.active) {
            *t = Pending { active: true, slot: s, due: self.frame + wait };
            self.slots[s].last_trigger = self.frame;
            self.pending_count += 1;
            return;
        }
        self.trigger(s); // queue full: better early than lost
    }

    fn fire_pending(&mut self) {
        for k in 0..PENDING {
            let t = self.pending[k];
            if !t.active || self.frame < t.due {
                continue;
            }
            self.pending[k].active = false;
            self.pending_count -= 1;
            if self.slots[t.slot].active {
                self.trigger(t.slot);
            }
        }
    }

    fn trigger(&mut self, s: usize) {
        self.slots[s].trigger_count = self.slots[s].trigger_count.wrapping_add(1);
        self.slots[s].last_trigger = self.frame;
        let ml = self.max_life();
        if !ml.is_infinite() {
            self.slots[s].life = min(self.slots[s].life + 0.5 * ml, 2.0 * ml);
        }
        self.total_triggers += 1;

        // Respect Density: release the oldest sounding voices beyond the cap.
        let cap = self.p.density.clamp(1, VOICES as i32) as usize;
        loop {
            let mut sounding = 0;
            let mut oldest: Option<usize> = None;
            for v in 0..VOICES {
                if !self.voices[v].active || self.voices[v].releasing {
                    continue;
                }
                sounding += 1;
                if oldest.is_none_or(|o| self.voices[v].start < self.voices[o].start) {
                    oldest = Some(v);
                }
            }
            match oldest {
                Some(o) if sounding >= cap => {
                    self.voices[o].releasing = true;
                    self.voices[o].env_inc = -self.release_inc;
                }
                _ => break,
            }
        }
        let free = match self.voices.iter().position(|v| !v.active) {
            Some(v) => v,
            None => {
                let mut f = 0;
                for v in 1..VOICES {
                    if self.voices[v].env < self.voices[f].env {
                        f = v;
                    }
                }
                f
            }
        };
        let angle = (self.rng.bipolar() * 0.4 + 1.0) * PI * 0.25;
        self.voices[free] = Voice {
            active: true,
            releasing: false,
            slot: s,
            pos: 0,
            env: 0.0,
            env_inc: 1.0 / (max(self.p.attack_sec, 0.0005) * self.sr),
            // Equal-power pan: a centred voice sits at -3 dB per side, like any mono source.
            gl: angle.cos(),
            gr: angle.sin(),
            meter: 0.0,
            start: self.frame,
        };
    }
}
