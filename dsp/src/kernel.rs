//! Top-level DSP kernel: parameters, sample engine → pre-delay → FDN → filter → mix, visual publishing.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};

use crate::engine::{ClockState, EngineParams, SampleEngine};
use crate::params::*;
use crate::reverb::{FdnReverb, ReverbParams};
use crate::triple_buffer::TripleBuffer;
use crate::util::{clampf, lround, min, DelayLine, Svf, PI};
use crate::visual::{SRSlotVisual, SR_MAX_SLOTS, SR_SCOPE_POINTS};

/// What the host reports at the start of a render block. Any part may be missing.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SRHostTime {
    pub has_tempo: bool,
    pub tempo: f64,
    pub has_position: bool,
    pub beat: f64,      // quarter notes
    pub downbeat: f64,  // beat of the current bar's first beat
    pub bar_beats: f64, // quarter notes per bar
    pub has_transport: bool,
    pub playing: bool,
}

impl Default for SRHostTime {
    fn default() -> Self {
        SRHostTime {
            has_tempo: false,
            tempo: 120.0,
            has_position: false,
            beat: 0.0,
            downbeat: 0.0,
            bar_beats: 4.0,
            has_transport: false,
            playing: false,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SRClockSummary {
    pub tempo: f64,
    pub beat: f64,
    pub from_host: bool,
}

pub struct VisualSummary {
    pub pool_count: i32,
    pub frozen: bool,
}

pub const CHUNK: usize = 256;

#[derive(Clone, Copy)]
struct VisualState {
    pool_count: i32,
    frozen: i32,
    slots: [SRSlotVisual; SR_MAX_SLOTS],
    scope_l: [f32; SR_SCOPE_POINTS],
    scope_r: [f32; SR_SCOPE_POINTS],
}

impl Default for VisualState {
    fn default() -> Self {
        VisualState {
            pool_count: 0,
            frozen: 0,
            slots: [SRSlotVisual::default(); SR_MAX_SLOTS],
            scope_l: [0.0; SR_SCOPE_POINTS],
            scope_r: [0.0; SR_SCOPE_POINTS],
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Smoothed {
    value: f32,
    previous: f32,
}

impl Smoothed {
    fn step(&mut self, target: f32, a: f32) {
        self.previous = self.value;
        self.value += (target - self.value) * a;
    }
    fn reset(&mut self, v: f32) {
        self.value = v;
        self.previous = v;
    }
    fn at(&self, t: f32) -> f32 {
        self.previous + (self.value - self.previous) * t
    }
}

/// State only the audio thread touches (and `prepare`, while rendering is stopped).
struct Render {
    sr: f32,
    first: bool,
    frozen: bool,
    mix: Smoothed,
    level: Smoothed,
    dry: Smoothed,
    filter: Smoothed,
    size: Smoothed,
    pre_cur: f32,
    host: SRHostTime,
    beat: f64,
    beats_per_frame: f64,
    bar_origin: f64,
    bar_beats: f64,
    grid_beats: f64,
    last_host_beat: f64,
    engine: SampleEngine,
    reverb: FdnReverb,
    pre_l: DelayLine,
    pre_r: DelayLine,
    filter_l: Svf,
    filter_r: Svf,
    scope_l: [f32; SR_SCOPE_POINTS],
    scope_r: [f32; SR_SCOPE_POINTS],
    scope_w: usize,
}

impl Default for Render {
    fn default() -> Self {
        Render {
            sr: 48000.0,
            first: true,
            frozen: false,
            mix: Smoothed::default(),
            level: Smoothed::default(),
            dry: Smoothed::default(),
            filter: Smoothed::default(),
            size: Smoothed::default(),
            pre_cur: 0.0,
            host: SRHostTime::default(),
            beat: 0.0,
            beats_per_frame: 0.0,
            bar_origin: 0.0,
            bar_beats: 4.0,
            grid_beats: 0.5,
            last_host_beat: -1.0,
            engine: SampleEngine::default(),
            reverb: FdnReverb::default(),
            pre_l: DelayLine::default(),
            pre_r: DelayLine::default(),
            filter_l: Svf::default(),
            filter_r: Svf::default(),
            scope_l: [0.0; SR_SCOPE_POINTS],
            scope_r: [0.0; SR_SCOPE_POINTS],
            scope_w: 0,
        }
    }
}

/// Parameter values as atomics, so any thread can set them while the audio thread reads them.
struct Params([AtomicU32; PARAM_COUNT]);

impl Params {
    fn get(&self, a: Param) -> f32 {
        f32::from_bits(self.0[a as usize].load(Relaxed))
    }
}

/// The kernel is shared between threads with these roles:
/// - any thread: `set_parameter`, `get_parameter`, `set_bypass`, `clock`
/// - the audio thread: `process` (and `prepare` while rendering is stopped)
/// - one UI thread: `read_visual`
pub struct Kernel {
    params: Params,
    bypass: AtomicBool,
    shown_tempo: AtomicU64,
    shown_beat: AtomicU64,
    shown_from_host: AtomicBool,
    visual: TripleBuffer<VisualState>,
    render: UnsafeCell<Render>,
}

// SAFETY: `render` is only accessed from `prepare` and `process`, whose contract is that one thread
// calls them at a time (the audio thread, or the main thread while rendering is stopped). Everything
// else is atomics or the single-producer/single-consumer triple buffer.
unsafe impl Sync for Kernel {}

impl Default for Kernel {
    fn default() -> Self {
        Self::new()
    }
}

impl Kernel {
    pub fn new() -> Kernel {
        Kernel {
            params: Params(std::array::from_fn(|i| AtomicU32::new(PARAM_SPECS[i].def.to_bits()))),
            bypass: AtomicBool::new(false),
            shown_tempo: AtomicU64::new(120f64.to_bits()),
            shown_beat: AtomicU64::new(0),
            shown_from_host: AtomicBool::new(false),
            visual: TripleBuffer::default(),
            render: UnsafeCell::new(Render::default()),
        }
    }

    // ---- Any thread -----------------------------------------------------------------------------

    pub fn set_parameter(&self, address: u64, value: f32) {
        if let Some(p) = self.params.0.get(address as usize) {
            p.store(value.to_bits(), Relaxed);
        }
    }

    pub fn get_parameter(&self, address: u64) -> f32 {
        self.params.0.get(address as usize).map_or(0.0, |p| f32::from_bits(p.load(Relaxed)))
    }

    pub fn set_bypass(&self, bypass: bool) {
        self.bypass.store(bypass, Relaxed);
    }

    pub fn clock(&self) -> SRClockSummary {
        SRClockSummary {
            tempo: f64::from_bits(self.shown_tempo.load(Relaxed)),
            beat: f64::from_bits(self.shown_beat.load(Relaxed)),
            from_host: self.shown_from_host.load(Relaxed),
        }
    }

    // ---- Non-real-time --------------------------------------------------------------------------

    /// Allocates every buffer for `sample_rate`.
    ///
    /// # Safety
    /// Must not run concurrently with `process` or another `prepare`.
    pub unsafe fn prepare(&self, sample_rate: f64) {
        let r = &mut *self.render.get();
        r.sr = sample_rate as f32;
        r.reverb.prepare(r.sr);
        r.engine.prepare(r.sr);
        r.pre_l.allocate(((MAX_PRE_DELAY_SEC + 0.01) * r.sr) as u32);
        r.pre_r.allocate(((MAX_PRE_DELAY_SEC + 0.01) * r.sr) as u32);
        r.filter_l.reset();
        r.filter_r.reset();
        r.scope_l = [0.0; SR_SCOPE_POINTS];
        r.scope_r = [0.0; SR_SCOPE_POINTS];
        r.scope_w = 0;
        r.first = true;
        r.beat = 0.0;
        r.bar_origin = 0.0;
        r.last_host_beat = -1.0;
    }

    // ---- UI thread (single consumer) ------------------------------------------------------------

    /// # Safety
    /// Only one thread may read visuals (the triple buffer has a single consumer).
    pub unsafe fn read_visual(
        &self,
        slots: &mut [SRSlotVisual],
        scope_l: &mut [f32],
        scope_r: &mut [f32],
    ) -> VisualSummary {
        let v = self.visual.read();
        let ns = slots.len().min(SR_MAX_SLOTS);
        slots[..ns].copy_from_slice(&v.slots[..ns]);
        let n = scope_l.len().min(scope_r.len()).min(SR_SCOPE_POINTS);
        scope_l[..n].copy_from_slice(&v.scope_l[SR_SCOPE_POINTS - n..]);
        scope_r[..n].copy_from_slice(&v.scope_r[SR_SCOPE_POINTS - n..]);
        VisualSummary { pool_count: v.pool_count, frozen: v.frozen != 0 }
    }

    // ---- Real-time ------------------------------------------------------------------------------

    /// Processes one block. Never allocates, locks or blocks.
    ///
    /// # Safety
    /// Must not run concurrently with itself or `prepare`, and `prepare` must have run first.
    pub unsafe fn process(&self, host: &SRHostTime, in_l: &[f32], in_r: &[f32], out_l: &mut [f32], out_r: &mut [f32]) {
        let frames = out_l.len().min(out_r.len()).min(in_l.len()).min(in_r.len());
        self.process_raw(host, in_l.as_ptr(), in_r.as_ptr(), out_l.as_mut_ptr(), out_r.as_mut_ptr(), frames);
    }

    /// `process` for host buffers, which may alias: the outputs may be the inputs (in-place), and
    /// the two inputs may be the same buffer (mono). Each chunk is copied in before any output is
    /// written, and no slice is ever formed over the host's memory.
    ///
    /// # Safety
    /// As `process`; also, each pointer must be valid for `frames` floats, and `out_l` and `out_r`
    /// must not overlap each other.
    pub unsafe fn process_raw(
        &self,
        host: &SRHostTime,
        in_l: *const f32,
        in_r: *const f32,
        out_l: *mut f32,
        out_r: *mut f32,
        frames: usize,
    ) {
        let r = &mut *self.render.get();
        r.host = *host;
        self.begin_clock(r);
        if self.bypass.load(Relaxed) {
            std::ptr::copy(in_l, out_l, frames);
            std::ptr::copy(in_r, out_r, frames);
            return;
        }
        let mut xl = [0f32; CHUNK];
        let mut xr = [0f32; CHUNK];
        let mut yl = [0f32; CHUNK];
        let mut yr = [0f32; CHUNK];
        let mut done = 0;
        while done < frames {
            let n = CHUNK.min(frames - done);
            std::ptr::copy(in_l.add(done), xl.as_mut_ptr(), n);
            std::ptr::copy(in_r.add(done), xr.as_mut_ptr(), n);
            r.engine.set_clock(ClockState {
                beat: r.beat,
                beats_per_frame: r.beats_per_frame,
                bar_origin: r.bar_origin,
            });
            self.process_chunk(r, &xl[..n], &xr[..n], &mut yl[..n], &mut yr[..n]);
            std::ptr::copy(yl.as_ptr(), out_l.add(done), n);
            std::ptr::copy(yr.as_ptr(), out_r.add(done), n);
            r.beat += n as f64 * r.beats_per_frame;
            done += n;
        }
        self.publish_visual(r);
    }

    /// Introspection for tests.
    ///
    /// # Safety
    /// Must not run concurrently with `process` or `prepare`.
    pub unsafe fn engine(&self) -> &SampleEngine {
        &(*self.render.get()).engine
    }

    fn p(&self, a: Param) -> f32 {
        self.params.get(a)
    }

    fn index(&self, a: Param, count: usize) -> usize {
        (lround(self.p(a)) as i32).max(0).min(count as i32 - 1) as usize
    }

    /// While the host's transport plays, follow its position. Otherwise run free at the host's
    /// tempo, or at the Tempo parameter when there is no host tempo (the standalone app).
    fn begin_clock(&self, r: &mut Render) {
        let host = r.host;
        let from_host = host.has_tempo && host.tempo > 0.0;
        let raw = if from_host { host.tempo } else { self.p(Param::Tempo) as f64 };
        let tempo = if raw < 20.0 { 20.0 } else { raw };
        let tempo = if 400.0 < tempo { 400.0 } else { tempo };
        r.beats_per_frame = tempo / (60.0 * r.sr as f64);
        r.bar_beats = if host.has_position && host.bar_beats > 0.0 { host.bar_beats } else { 4.0 };
        // Without transport state, the position only counts as playing while it moves.
        let playing =
            host.has_position && if host.has_transport { host.playing } else { host.beat != r.last_host_beat };
        r.last_host_beat = if host.has_position { host.beat } else { -1.0 };
        if playing {
            r.beat = host.beat;
            r.bar_origin = host.downbeat;
        }
        r.grid_beats = division_beats(GRID_BEATS[self.index(Param::Grid, GRID_BEATS.len())], r.bar_beats);
        self.shown_tempo.store(tempo.to_bits(), Relaxed);
        self.shown_beat.store((r.beat - r.bar_origin).to_bits(), Relaxed);
        self.shown_from_host.store(from_host, Relaxed);
    }

    /// The length of an indexed note-division parameter at the current tempo.
    fn synced_seconds(&self, r: &Render, a: Param, beats: &[f64]) -> f32 {
        (division_beats(beats[self.index(a, beats.len())], r.bar_beats) / (r.beats_per_frame * r.sr as f64)) as f32
    }

    fn process_chunk(&self, r: &mut Render, in_l: &[f32], in_r: &[f32], out_l: &mut [f32], out_r: &mut [f32]) {
        let n = out_l.len();
        let freeze = self.p(Param::Freeze) >= 0.5;
        r.frozen = freeze;
        let mix_t = clampf(self.p(Param::Mix) / 100.0, 0.0, 1.0);
        let level_t = clampf(self.p(Param::SampleLevel) / 100.0, 0.0, 1.0);
        let dry_t = clampf(self.p(Param::DryFeed) / 100.0, 0.0, 1.0);
        let filter_t = clampf(self.p(Param::Filter) / 100.0, -1.0, 1.0);
        let sync = self.p(Param::Sync) >= 0.5;
        let pre_sec = if sync {
            self.synced_seconds(r, Param::PreDelayDiv, &PRE_DELAY_DIV_BEATS)
        } else {
            clampf(self.p(Param::PreDelay), 0.0, 500.0) * 0.001
        };
        let pre_t = min(pre_sec, MAX_PRE_DELAY_SEC) * r.sr;
        let size_t = clampf(self.p(Param::Size) / 100.0, 0.0, 1.0);

        if r.first {
            r.mix.reset(mix_t);
            r.level.reset(level_t);
            r.dry.reset(dry_t);
            r.filter.reset(filter_t);
            r.size.reset(size_t);
            r.pre_cur = pre_t;
            r.first = false;
        } else {
            let a = 1.0 - (-(n as f32) / (0.03 * r.sr)).exp();
            r.mix.step(mix_t, a);
            r.level.step(level_t, a);
            r.dry.step(dry_t, a);
            r.filter.step(filter_t, a);
            r.size.step(size_t, a);
        }

        let ep = EngineParams {
            retention: self.p(Param::Retention) / 100.0,
            sensitivity_db: self.p(Param::Sensitivity),
            match_tolerance: self.p(Param::Match) / 100.0,
            harmony: self.p(Param::Harmony) / 100.0,
            length_sec: if sync {
                self.synced_seconds(r, Param::LengthDiv, &LENGTH_DIV_BEATS)
            } else {
                self.p(Param::Length)
            },
            attack_sec: self.p(Param::Attack) * 0.001,
            density: lround(self.p(Param::Density)) as i32,
            focus_x: self.p(Param::FocusX),
            focus_y: self.p(Param::FocusY),
            focus_amount: self.p(Param::FocusAmount) / 100.0,
            freeze,
            quantize: self.p(Param::Quantize) >= 0.5,
            grid_beats: r.grid_beats,
        };
        r.engine.set_params(&ep);

        let rp = ReverbParams {
            decay_sec: self.p(Param::Decay),
            size: r.size.value,
            tone: self.p(Param::Tone) / 100.0,
            texture: self.p(Param::Texture) / 100.0,
            modulation: self.p(Param::Mod) / 100.0,
            shimmer: self.p(Param::Shimmer) / 100.0,
            shimmer_semitones: INTERVAL_SEMITONES[self.index(Param::Interval, INTERVAL_SEMITONES.len())],
            sub: self.p(Param::Sub) / 100.0,
            freeze,
        };
        r.reverb.set_params(&rp);

        // The caller has already copied the input, so it's safe to read while writing the output.
        let (xl, xr) = (in_l, in_r);
        let mut vl = [0f32; CHUNK];
        let mut vr = [0f32; CHUNK];
        let mut rl = [0f32; CHUNK];
        let mut rr = [0f32; CHUNK];
        let mut wl = [0f32; CHUNK];
        let mut wr = [0f32; CHUNK];
        r.engine.process(xl, xr, &mut vl[..n], &mut vr[..n]);

        let pre_coef = 1.0 - (-1.0 / (0.05 * r.sr)).exp();
        for i in 0..n {
            let t = (i + 1) as f32 / n as f32;
            let (dry, level) = (r.dry.at(t), r.level.at(t));
            r.pre_l.write(xl[i] * dry + vl[i] * level);
            r.pre_r.write(xr[i] * dry + vr[i] * level);
            r.pre_cur += (pre_t - r.pre_cur) * pre_coef;
            rl[i] = r.pre_l.read(r.pre_cur);
            rr[i] = r.pre_r.read(r.pre_cur);
        }

        r.reverb.process(&rl[..n], &rr[..n], &mut wl[..n], &mut wr[..n]);

        // Bipolar filter: left = low-pass sweep 20 kHz → 200 Hz, right = high-pass 20 Hz → 4 kHz.
        let f = r.filter.value;
        let highpass = f > 0.0;
        let cutoff = min(if highpass { 20.0 * 200f32.powf(f) } else { 20000.0 * 0.01f32.powf(-f) }, 0.45 * r.sr);
        let res = clampf(self.p(Param::Resonance) / 100.0, 0.0, 1.0);
        let q = 0.707 + res * 9.3 * min(1.0, f.abs() * 5.0);
        r.filter_l.set(cutoff, q, r.sr);
        r.filter_r.set(cutoff, q, r.sr);

        for i in 0..n {
            let (lp, hp) = r.filter_l.process(wl[i]);
            let yl = if highpass { hp } else { lp };
            let (lp, hp) = r.filter_r.process(wr[i]);
            let yr = if highpass { hp } else { lp };

            r.scope_l[r.scope_w] = yl;
            r.scope_r[r.scope_w] = yr;
            r.scope_w = (r.scope_w + 1) & (SR_SCOPE_POINTS - 1);

            let angle = r.mix.at((i + 1) as f32 / n as f32) * PI * 0.5;
            let (dry_gain, wet_gain) = (angle.cos(), angle.sin());
            out_l[i] = xl[i] * dry_gain + yl * wet_gain;
            out_r[i] = xr[i] * dry_gain + yr * wet_gain;
        }

        r.engine.advance_life(n as f32 / r.sr);
    }

    fn publish_visual(&self, r: &Render) {
        // SAFETY: `process` is the single producer.
        let v = unsafe { self.visual.back_buffer() };
        v.pool_count = r.engine.pool_count();
        v.frozen = r.frozen as i32;
        r.engine.fill_visual(&mut v.slots);
        let head = SR_SCOPE_POINTS - r.scope_w;
        v.scope_l[..head].copy_from_slice(&r.scope_l[r.scope_w..]);
        v.scope_l[head..].copy_from_slice(&r.scope_l[..r.scope_w]);
        v.scope_r[..head].copy_from_slice(&r.scope_r[r.scope_w..]);
        v.scope_r[head..].copy_from_slice(&r.scope_r[..r.scope_w]);
        unsafe { self.visual.publish() };
    }
}

const _: () = assert!(SR_SCOPE_POINTS.is_power_of_two(), "scope ring must be a power of two");
