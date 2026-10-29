//! Sampling Reverb's real-time DSP core.
//!
//! A port of the original C++ kernel (Extension/DSP) that produces bit-identical output. The AU
//! talks to it through the C ABI in `ffi.rs` (header: include/sr_dsp.h).

// The constants and loops mirror the C++ reference literally so the output stays bit-identical:
// its π and √2 literals, and index loops over parallel arrays.
#![allow(clippy::approx_constant, clippy::excessive_precision, clippy::needless_range_loop)]

pub mod engine;
pub mod ffi;
pub mod kernel;
pub mod params;
pub mod reverb;
pub mod triple_buffer;
pub mod util;
pub mod visual;

pub use kernel::{Kernel, SRClockSummary, SRHostTime};
