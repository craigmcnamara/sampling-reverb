//! Sampling Reverb's real-time DSP core.
//!
//! The AU talks to it through the C ABI in `ffi.rs` (header: include/sr_dsp.h). It began as a
//! line-for-line port of the original C++ kernel, verified bit-identical before the C++ was removed.

// Constants and loops kept as they were in the original C++ (its π and √2 literals, index loops
// over parallel arrays), so the sound is unchanged from it.
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
