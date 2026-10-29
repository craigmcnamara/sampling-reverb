//! Parameter metadata: the single source of truth for ranges, defaults, value names and presets.
//! Addresses match `SRParam` in Extension/DSP/ParamIDs.h, which Swift and Objective-C share.
//! Names are C strings because the AU reads them through the C ABI.

use std::ffi::CStr;

#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unit {
    Percent = 0,
    Seconds,
    Milliseconds,
    Decibels,
    Indexed,
    Boolean,
    Bpm,
    Generic,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u64)]
pub enum Param {
    Mix = 0,
    Decay,
    Size,
    PreDelay,
    Tone,
    Texture,
    Mod,
    Shimmer,
    Interval,
    Filter,
    Resonance,
    Freeze,
    Retention,
    Sensitivity,
    Match,
    Harmony,
    Length,
    SampleLevel,
    Density,
    DryFeed,
    FocusX,
    FocusY,
    FocusAmount,
    // Added after 0.1: new parameters go at the end so saved states keep their addresses.
    Sub,
    Attack,
    Quantize,
    Grid,
    Tempo,
    Sync,
    PreDelayDiv,
    LengthDiv,
}

pub const PARAM_COUNT: usize = 31;

pub struct ParamSpec {
    pub address: Param,
    pub identifier: &'static CStr,
    pub name: &'static CStr,
    pub min: f32,
    pub max: f32,
    pub def: f32,
    pub unit: Unit,
    pub exponential: bool,
}

pub const INTERVAL_SEMITONES: [f32; 6] = [-12.0, -5.0, 5.0, 7.0, 12.0, 19.0];
pub const INTERVAL_NAMES: [&CStr; 6] = [c"-12", c"-5", c"+5", c"+7", c"+12", c"+19"];

/// Note divisions are in quarter-note beats; a negative value -n means n bars of the current time
/// signature.
#[inline]
pub fn division_beats(division: f64, bar_beats: f64) -> f64 {
    if division < 0.0 {
        -division * bar_beats
    } else {
        division
    }
}

pub const GRID_BEATS: [f64; 8] = [0.125, 0.25, 1.0 / 3.0, 0.5, 2.0 / 3.0, 1.0, 2.0, -1.0];
pub const GRID_NAMES: [&CStr; 8] = [c"1/32", c"1/16", c"1/8T", c"1/8", c"1/4T", c"1/4", c"1/2", c"1 Bar"];

/// Synced Pre-delay: T = triplet, D = dotted.
pub const PRE_DELAY_DIV_BEATS: [f64; 13] = [
    1.0 / 16.0,
    1.0 / 12.0,
    1.0 / 8.0,
    1.0 / 6.0,
    1.0 / 4.0,
    3.0 / 8.0,
    1.0 / 3.0,
    1.0 / 2.0,
    3.0 / 4.0,
    2.0 / 3.0,
    1.0,
    3.0 / 2.0,
    2.0,
];
pub const PRE_DELAY_DIV_NAMES: [&CStr; 13] = [
    c"1/64", c"1/32T", c"1/32", c"1/16T", c"1/16", c"1/16D", c"1/8T", c"1/8", c"1/8D", c"1/4T", c"1/4", c"1/4D", c"1/2",
];
/// A half note at 40 BPM.
pub const MAX_PRE_DELAY_SEC: f32 = 3.0;

pub const LENGTH_DIV_BEATS: [f64; 7] = [0.5, 1.0, 2.0, -1.0, -2.0, -4.0, -8.0];
pub const LENGTH_DIV_NAMES: [&CStr; 7] = [c"1/8", c"1/4", c"1/2", c"1 Bar", c"2 Bars", c"4 Bars", c"8 Bars"];

macro_rules! spec {
    ($p:ident, $id:literal, $name:literal, $min:expr, $max:expr, $def:expr, $unit:ident, $exp:literal) => {
        ParamSpec {
            address: Param::$p,
            identifier: $id,
            name: $name,
            min: $min,
            max: $max,
            def: $def,
            unit: Unit::$unit,
            exponential: $exp,
        }
    };
}

#[rustfmt::skip]
pub const PARAM_SPECS: [ParamSpec; PARAM_COUNT] = [
    spec!(Mix,         c"mix",         c"Mix",            0.0,    100.0,  50.0,  Percent,      false),
    spec!(Decay,       c"decay",       c"Decay",          0.3,    30.0,   4.0,   Seconds,      true),
    spec!(Size,        c"size",        c"Size",           0.0,    100.0,  60.0,  Percent,      false),
    spec!(PreDelay,    c"predelay",    c"Pre-delay",      0.0,    500.0,  20.0,  Milliseconds, false),
    spec!(Tone,        c"tone",        c"Tone",           0.0,    100.0,  60.0,  Percent,      false),
    spec!(Texture,     c"texture",     c"Texture",        0.0,    100.0,  50.0,  Percent,      false),
    spec!(Mod,         c"mod",         c"Mod",            0.0,    100.0,  30.0,  Percent,      false),
    spec!(Shimmer,     c"shimmer",     c"Shimmer",        0.0,    100.0,  0.0,   Percent,      false),
    spec!(Interval,    c"interval",    c"Interval",       0.0,    5.0,    4.0,   Indexed,      false),
    spec!(Filter,      c"filter",      c"Filter",        -100.0,  100.0,  0.0,   Generic,      false),
    spec!(Resonance,   c"resonance",   c"Resonance",      0.0,    100.0,  20.0,  Percent,      false),
    spec!(Freeze,      c"freeze",      c"Freeze",         0.0,    1.0,    0.0,   Boolean,      false),
    spec!(Retention,   c"retention",   c"Retention",      0.0,    100.0,  50.0,  Percent,      false),
    spec!(Sensitivity, c"sensitivity", c"Sensitivity",   -60.0,  -20.0,  -40.0,  Decibels,     false),
    spec!(Match,       c"match",       c"Match",          0.0,    100.0,  40.0,  Percent,      false),
    spec!(Harmony,     c"harmony",     c"Harmony",        0.0,    100.0,  30.0,  Percent,      false),
    spec!(Length,      c"length",      c"Length",         0.1,    20.0,   3.0,   Seconds,      true),
    spec!(SampleLevel, c"samplelevel", c"Sample Level",   0.0,    100.0,  80.0,  Percent,      false),
    spec!(Density,     c"density",     c"Density",        1.0,    8.0,    4.0,   Generic,      false),
    spec!(DryFeed,     c"dryfeed",     c"Dry Feed",       0.0,    100.0,  50.0,  Percent,      false),
    spec!(FocusX,      c"focusx",      c"Focus X",        0.0,    1.0,    0.5,   Generic,      false),
    spec!(FocusY,      c"focusy",      c"Focus Y",        0.0,    1.0,    0.5,   Generic,      false),
    spec!(FocusAmount, c"focus",       c"Focus",          0.0,    100.0,  0.0,   Percent,      false),
    spec!(Sub,         c"sub",         c"Sub",            0.0,    100.0,  0.0,   Percent,      false),
    spec!(Attack,      c"attack",      c"Attack",         1.0,    2000.0, 100.0, Milliseconds, true),
    spec!(Quantize,    c"quantize",    c"Quantize",       0.0,    1.0,    0.0,   Boolean,      false),
    spec!(Grid,        c"grid",        c"Grid",           0.0,    7.0,    3.0,   Indexed,      false),
    spec!(Tempo,       c"tempo",       c"Tempo",          40.0,   240.0,  120.0, Bpm,          false),
    spec!(Sync,        c"sync",        c"Sync",           0.0,    1.0,    0.0,   Boolean,      false),
    spec!(PreDelayDiv, c"predelaydiv", c"Pre-delay Sync", 0.0,    12.0,   2.0,   Indexed,      false),
    spec!(LengthDiv,   c"lengthdiv",   c"Length Sync",    0.0,    6.0,    3.0,   Indexed,      false),
];

/// The display names of an indexed parameter's values, if it has them.
pub fn value_names(address: u64) -> &'static [&'static CStr] {
    match address {
        a if a == Param::Interval as u64 => &INTERVAL_NAMES,
        a if a == Param::Grid as u64 => &GRID_NAMES,
        a if a == Param::PreDelayDiv as u64 => &PRE_DELAY_DIV_NAMES,
        a if a == Param::LengthDiv as u64 => &LENGTH_DIV_NAMES,
        _ => &[],
    }
}

/// Performance controls: presets leave these alone (Freeze, and the clock, which belongs to the song).
pub fn preset_leaves_alone(address: u64) -> bool {
    [Param::Freeze, Param::Quantize, Param::Grid, Param::Tempo, Param::Sync].iter().any(|p| *p as u64 == address)
}

pub struct Preset {
    pub name: &'static CStr,
    pub values: [f32; PARAM_COUNT],
}

const fn default_values() -> [f32; PARAM_COUNT] {
    let mut v = [0.0; PARAM_COUNT];
    let mut i = 0;
    while i < PARAM_COUNT {
        v[PARAM_SPECS[i].address as usize] = PARAM_SPECS[i].def;
        i += 1;
    }
    v
}

/// Presets don't choose the performance controls; store their defaults.
const fn with_performance_defaults(name: &'static CStr, mut values: [f32; PARAM_COUNT]) -> Preset {
    let d = default_values();
    let ignored = [Param::Freeze, Param::Quantize, Param::Grid, Param::Tempo, Param::Sync];
    let mut i = 0;
    while i < ignored.len() {
        values[ignored[i] as usize] = d[ignored[i] as usize];
        i += 1;
    }
    Preset { name, values }
}

pub const PRESET_COUNT: usize = 7;

#[rustfmt::skip]
pub const PRESETS: [Preset; PRESET_COUNT] = [
    // "Default" is built from the parameter table so it can never drift from the real defaults.
    Preset { name: c"Default", values: default_values() },
    //                                       Mix   Decay Size   Pre   Tone  Tex   Mod   Shim  Int  Filt   Res   Frz  Ret    Sens   Match Harm  Len   Lvl    Dens Dry   FX   FY   Focus Sub   Atk    Qnt  Grid Tempo  Sync PreDiv LenDiv
    with_performance_defaults(c"Night Sky",       [55.0, 12.0, 85.0,  40.0, 45.0, 80.0, 30.0, 35.0, 4.0, -10.0,  0.0, 0.0, 60.0,  -40.0, 40.0, 40.0, 6.0,  80.0,  4.0, 40.0, 0.5, 0.5, 0.0,  35.0, 400.0, 0.0, 3.0, 120.0, 0.0, 2.0,   4.0]),
    with_performance_defaults(c"Echo Garden",     [60.0, 5.0,  60.0,  80.0, 65.0, 30.0, 25.0, 0.0,  4.0,  0.0,  20.0, 0.0, 80.0,  -45.0, 55.0, 85.0, 8.0,  100.0, 6.0, 0.0,  0.5, 0.5, 0.0,  0.0,  60.0,  0.0, 3.0, 120.0, 0.0, 3.0,   5.0]),
    with_performance_defaults(c"Dark Matter",     [50.0, 20.0, 100.0, 10.0, 25.0, 70.0, 60.0, 0.0,  0.0, -55.0, 35.0, 0.0, 50.0,  -40.0, 40.0, 20.0, 5.0,  70.0,  4.0, 50.0, 0.5, 0.5, 0.0,  65.0, 800.0, 0.0, 3.0, 120.0, 0.0, 0.0,   4.0]),
    with_performance_defaults(c"Glass Choir",     [50.0, 8.0,  70.0,  30.0, 90.0, 85.0, 35.0, 60.0, 3.0,  35.0, 25.0, 0.0, 40.0,  -40.0, 45.0, 70.0, 3.0,  70.0,  5.0, 50.0, 0.5, 0.5, 0.0,  0.0,  250.0, 0.0, 3.0, 120.0, 0.0, 0.0,   3.0]),
    with_performance_defaults(c"Short Memory",    [40.0, 1.5,  30.0,  0.0,  60.0, 40.0, 15.0, 0.0,  4.0,  0.0,  20.0, 0.0, 0.0,   -35.0, 50.0, 30.0, 0.4,  90.0,  8.0, 60.0, 0.5, 0.5, 0.0,  0.0,  10.0,  0.0, 3.0, 120.0, 0.0, 0.0,   1.0]),
    with_performance_defaults(c"Infinite Archive",[50.0, 25.0, 90.0,  60.0, 50.0, 60.0, 40.0, 20.0, 5.0,  0.0,  20.0, 0.0, 100.0, -50.0, 25.0, 50.0, 20.0, 80.0,  3.0, 30.0, 0.5, 0.5, 0.0,  25.0, 600.0, 0.0, 3.0, 120.0, 0.0, 2.0,   6.0]),
];
