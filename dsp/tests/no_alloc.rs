//! The render path must never allocate: a counting global allocator fails the test if `process`
//! touches the heap, across every preset, Sync, Quantize, host time and block size.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use common::*;
use sampling_reverb_dsp::kernel::{Kernel, SRHostTime};
use sampling_reverb_dsp::params::{Param, PRESETS};

struct Counting;

thread_local! {
    static WATCHING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if WATCHING.with(|w| w.get()) {
            ALLOCATIONS.with(|a| a.set(a.get() + 1));
        }
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if WATCHING.with(|w| w.get()) {
            ALLOCATIONS.with(|a| a.set(a.get() + 1));
        }
        System.dealloc(ptr, layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if WATCHING.with(|w| w.get()) {
            ALLOCATIONS.with(|a| a.set(a.get() + 1));
        }
        System.realloc(ptr, layout, new_size)
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

#[test]
fn process_never_allocates() {
    const SR: f32 = 48000.0;
    let kernel = Kernel::new();
    unsafe { kernel.prepare(SR as f64) };
    let mut input = Buffer::new();
    for i in 0..30 {
        append(&mut input, &strum(0.6, 0.5, SR));
        append(&mut input, &tone_burst(110.0 * (1 + i % 5) as f32, 0.3, 0.5, SR));
    }
    let mut out_l = vec![0.0; 4096];
    let mut out_r = vec![0.0; 4096];
    let mut i = 0;
    let mut block = 1;
    let mut step = 0;
    while i < input.len() {
        let n = block.min(input.len() - i);
        if step % 50 == 0 {
            let preset = &PRESETS[(step / 50) % PRESETS.len()];
            for (p, v) in preset.values.iter().enumerate() {
                kernel.set_parameter(p as u64, *v);
            }
            kernel.set_parameter(Param::Sync as u64, ((step / 50) % 2) as f32);
            kernel.set_parameter(Param::Quantize as u64, ((step / 100) % 2) as f32);
            kernel.set_parameter(Param::Tempo as u64, 97.3);
        }
        let host = SRHostTime {
            has_tempo: step % 3 != 0,
            tempo: 133.33,
            has_position: true,
            beat: i as f64 / 20000.0,
            playing: step % 7 != 0,
            has_transport: true,
            ..Default::default()
        };
        WATCHING.with(|w| w.set(true));
        unsafe { kernel.process(&host, &input[i..i + n], &input[i..i + n], &mut out_l[..n], &mut out_r[..n]) };
        WATCHING.with(|w| w.set(false));
        i += n;
        block = (block + 37) % 4096 + 1;
        step += 1;
    }
    assert_eq!(ALLOCATIONS.with(|a| a.get()), 0, "process allocated");
}
