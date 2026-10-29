//! Lock-free single-producer / single-consumer "latest value" exchange.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU8, Ordering};

const INDEX: u8 = 3;
const DIRTY: u8 = 4;

pub struct TripleBuffer<T> {
    buffers: [UnsafeCell<T>; 3],
    middle: AtomicU8,
    back: UnsafeCell<u8>,
    front: UnsafeCell<u8>,
}

// SAFETY: the producer only touches `back` and the buffer it indexes; the consumer only touches
// `front` and its buffer. Ownership of buffers moves between them through the atomic `middle`
// exchange (acquire/release), so no buffer is ever accessed from two threads at once, provided
// there is a single producer and a single consumer (the contract of `back_buffer`/`read`).
unsafe impl<T: Send> Sync for TripleBuffer<T> {}

impl<T: Default> Default for TripleBuffer<T> {
    fn default() -> Self {
        TripleBuffer {
            buffers: [UnsafeCell::new(T::default()), UnsafeCell::new(T::default()), UnsafeCell::new(T::default())],
            middle: AtomicU8::new(1),
            back: UnsafeCell::new(0),
            front: UnsafeCell::new(2),
        }
    }
}

impl<T> TripleBuffer<T> {
    /// Producer: the buffer to fill before `publish`.
    ///
    /// # Safety
    /// Only the single producer thread may call this and `publish`, and it must not keep the
    /// reference past `publish`.
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn back_buffer(&self) -> &mut T {
        &mut *self.buffers[*self.back.get() as usize].get()
    }

    /// # Safety
    /// Producer thread only (see `back_buffer`).
    pub unsafe fn publish(&self) {
        let back = self.back.get();
        *back = self.middle.swap(*back | DIRTY, Ordering::AcqRel) & INDEX;
    }

    /// Consumer: the most recently published value.
    ///
    /// # Safety
    /// Only the single consumer thread may call this, and it must not keep the reference past its
    /// next call.
    pub unsafe fn read(&self) -> &T {
        let front = self.front.get();
        if self.middle.load(Ordering::Acquire) & DIRTY != 0 {
            *front = self.middle.swap(*front, Ordering::AcqRel) & INDEX;
        }
        &*self.buffers[*front as usize].get()
    }
}
