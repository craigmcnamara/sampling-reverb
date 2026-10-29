//! Plain C types the kernel publishes for the UI; must match Extension/DSP/VisualTypes.h.

pub const SR_MAX_SLOTS: usize = 24;
pub const SR_SCOPE_POINTS: usize = 1024;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct SRSlotVisual {
    pub active: i32,
    pub generation: u32,    // increments each time the slot is refilled
    pub trigger_count: u32, // increments each time the slot is played
    pub x: f32,             // brightness, 0…1
    pub y: f32,             // chroma (pitched) or loudness (unpitched), 0…1
    pub life: f32,          // remaining life / max life, 0…2
    pub clarity: f32,       // 0 = noisy, 1 = clearly pitched
    pub level: f32,         // current playback level, 0…1
}
