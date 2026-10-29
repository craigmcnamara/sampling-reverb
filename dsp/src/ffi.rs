//! The C ABI the Audio Unit uses (declared in include/sr_dsp.h).
//!
//! Every function tolerates a null kernel pointer, and indexes are range-checked, so a bad call
//! from the host side returns a neutral value instead of reading out of bounds.

use std::ffi::c_char;
use std::ptr;

use crate::kernel::{Kernel, SRClockSummary, SRHostTime};
use crate::params::{preset_leaves_alone, value_names, PARAM_COUNT, PARAM_SPECS, PRESETS, PRESET_COUNT};
use crate::visual::{SRSlotVisual, SR_MAX_SLOTS};

/// Opaque handle; C sees `SRKernel *`.
pub struct SRKernel(Kernel);

#[repr(C)]
pub struct SRParamSpec {
    pub identifier: *const c_char,
    pub name: *const c_char,
    pub min: f32,
    pub max: f32,
    pub def: f32,
    pub unit: u32,
    pub exponential: bool,
}

// ---- Lifetime (main thread) ----------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn sr_kernel_new() -> *mut SRKernel {
    Box::into_raw(Box::new(SRKernel(Kernel::new())))
}

/// # Safety
/// `k` must come from `sr_kernel_new` and not be used afterwards; nothing may be rendering.
#[no_mangle]
pub unsafe extern "C" fn sr_kernel_free(k: *mut SRKernel) {
    if !k.is_null() {
        drop(Box::from_raw(k));
    }
}

/// Allocates all buffers. Call while not rendering (e.g. in allocateRenderResources).
///
/// # Safety
/// `k` must be a live kernel; must not run concurrently with `sr_kernel_process`.
#[no_mangle]
pub unsafe extern "C" fn sr_kernel_prepare(k: *const SRKernel, sample_rate: f64) {
    if let Some(k) = k.as_ref() {
        if sample_rate.is_finite() && sample_rate > 0.0 {
            k.0.prepare(sample_rate);
        }
    }
}

// ---- Any thread ---------------------------------------------------------------------------------

/// # Safety
/// `k` must be null or a live kernel.
#[no_mangle]
pub unsafe extern "C" fn sr_kernel_set_parameter(k: *const SRKernel, address: u64, value: f32) {
    if let Some(k) = k.as_ref() {
        k.0.set_parameter(address, value);
    }
}

/// # Safety
/// `k` must be null or a live kernel.
#[no_mangle]
pub unsafe extern "C" fn sr_kernel_get_parameter(k: *const SRKernel, address: u64) -> f32 {
    k.as_ref().map_or(0.0, |k| k.0.get_parameter(address))
}

/// # Safety
/// `k` must be null or a live kernel.
#[no_mangle]
pub unsafe extern "C" fn sr_kernel_set_bypass(k: *const SRKernel, bypass: bool) {
    if let Some(k) = k.as_ref() {
        k.0.set_bypass(bypass);
    }
}

/// # Safety
/// `k` must be null or a live kernel.
#[no_mangle]
pub unsafe extern "C" fn sr_kernel_clock(k: *const SRKernel) -> SRClockSummary {
    k.as_ref().map_or(SRClockSummary { tempo: 120.0, beat: 0.0, from_host: false }, |k| k.0.clock())
}

// ---- Audio thread -------------------------------------------------------------------------------

/// Processes `frames` frames. The outputs may be the inputs (in-place) and `in_l` may equal `in_r`.
///
/// # Safety
/// `k` must be a prepared, live kernel, called from one thread at a time; every buffer must hold
/// `frames` floats, and `out_l` and `out_r` must not overlap. `host` may be null.
#[no_mangle]
pub unsafe extern "C" fn sr_kernel_process(
    k: *const SRKernel,
    host: *const SRHostTime,
    in_l: *const f32,
    in_r: *const f32,
    out_l: *mut f32,
    out_r: *mut f32,
    frames: i32,
) {
    let Some(k) = k.as_ref() else { return };
    if frames <= 0 || in_l.is_null() || in_r.is_null() || out_l.is_null() || out_r.is_null() {
        return;
    }
    let host = host.as_ref().copied().unwrap_or_default();
    k.0.process_raw(&host, in_l, in_r, out_l, out_r, frames as usize);
}

// ---- UI thread (single consumer) ----------------------------------------------------------------

/// Copies the latest visual snapshot. Returns the pool count.
///
/// # Safety
/// `k` must be null or a live kernel, read by one thread only; `slots` must hold `slot_count`
/// entries and each scope buffer `scope_count` floats (either may be null).
#[no_mangle]
pub unsafe extern "C" fn sr_kernel_read_visual(
    k: *const SRKernel,
    slots: *mut SRSlotVisual,
    slot_count: i32,
    scope_l: *mut f32,
    scope_r: *mut f32,
    scope_count: i32,
    frozen: *mut bool,
) -> i32 {
    let Some(k) = k.as_ref() else { return 0 };
    let mut slot_buf = [SRSlotVisual::default(); SR_MAX_SLOTS];
    let ns = (slot_count.max(0) as usize).min(SR_MAX_SLOTS);
    let nscope = scope_count.max(0) as usize;
    let (mut empty_l, mut empty_r) = ([0f32; 0], [0f32; 0]);
    let (sl, sr): (&mut [f32], &mut [f32]) = if scope_l.is_null() || scope_r.is_null() {
        (&mut empty_l, &mut empty_r)
    } else {
        (std::slice::from_raw_parts_mut(scope_l, nscope), std::slice::from_raw_parts_mut(scope_r, nscope))
    };
    let summary = k.0.read_visual(&mut slot_buf[..ns], sl, sr);
    if !slots.is_null() {
        ptr::copy_nonoverlapping(slot_buf.as_ptr(), slots, ns);
    }
    if !frozen.is_null() {
        *frozen = summary.frozen;
    }
    summary.pool_count
}

// ---- Metadata (any thread) ----------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn sr_param_count() -> i32 {
    PARAM_COUNT as i32
}

/// The spec for `address`, or a zeroed spec with null names for an unknown address.
#[no_mangle]
pub extern "C" fn sr_param_spec(address: u64) -> SRParamSpec {
    match PARAM_SPECS.get(address as usize) {
        Some(s) => SRParamSpec {
            identifier: s.identifier.as_ptr(),
            name: s.name.as_ptr(),
            min: s.min,
            max: s.max,
            def: s.def,
            unit: s.unit as u32,
            exponential: s.exponential,
        },
        None => SRParamSpec {
            identifier: ptr::null(),
            name: ptr::null(),
            min: 0.0,
            max: 0.0,
            def: 0.0,
            unit: 0,
            exponential: false,
        },
    }
}

/// Number of named values for an indexed parameter (0 if it has none).
#[no_mangle]
pub extern "C" fn sr_param_value_name_count(address: u64) -> i32 {
    value_names(address).len() as i32
}

/// The name of value `index` of an indexed parameter, or null.
#[no_mangle]
pub extern "C" fn sr_param_value_name(address: u64, index: i32) -> *const c_char {
    usize::try_from(index).ok().and_then(|i| value_names(address).get(i)).map_or(ptr::null(), |n| n.as_ptr())
}

#[no_mangle]
pub extern "C" fn sr_preset_count() -> i32 {
    PRESET_COUNT as i32
}

/// The name of preset `index`, or null.
#[no_mangle]
pub extern "C" fn sr_preset_name(index: i32) -> *const c_char {
    usize::try_from(index).ok().and_then(|i| PRESETS.get(i)).map_or(ptr::null(), |p| p.name.as_ptr())
}

/// Preset `index`'s value for `address` (0 if either is out of range).
#[no_mangle]
pub extern "C" fn sr_preset_value(index: i32, address: u64) -> f32 {
    usize::try_from(index)
        .ok()
        .and_then(|i| PRESETS.get(i))
        .and_then(|p| p.values.get(address as usize))
        .copied()
        .unwrap_or(0.0)
}

/// Performance controls (Freeze and the clock) that presets don't set.
#[no_mangle]
pub extern "C" fn sr_preset_leaves_alone(address: u64) -> bool {
    preset_leaves_alone(address)
}
