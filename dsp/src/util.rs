//! Small real-time-safe DSP building blocks.

pub const PI: f32 = 3.14159265358979;
pub const TWO_PI: f32 = 2.0 * PI;

/// `std::max` semantics: returns `a` unless `a < b` (so a NaN `a` passes through).
#[inline]
pub fn max(a: f32, b: f32) -> f32 {
    if a < b {
        b
    } else {
        a
    }
}

/// `std::min` semantics: returns `a` unless `b < a`.
#[inline]
pub fn min(a: f32, b: f32) -> f32 {
    if b < a {
        b
    } else {
        a
    }
}

#[inline]
pub fn clampf(x: f32, lo: f32, hi: f32) -> f32 {
    min(max(x, lo), hi)
}

#[inline]
pub fn lerpf(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[inline]
pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

#[inline]
pub fn gain_to_db(g: f32) -> f32 {
    20.0 * max(g, 1e-9).log10()
}

/// C's `lroundf`: round half away from zero.
#[inline]
pub fn lround(x: f32) -> i64 {
    x.round() as i64
}

pub fn next_pow2(v: u32) -> u32 {
    let mut n = 1u32;
    while n < v {
        n <<= 1;
    }
    n
}

/// Transparent below |x| = 1, then saturates smoothly towards ±2. Keeps feedback loops bounded.
#[inline]
pub fn safe_limit(x: f32) -> f32 {
    let a = x.abs();
    if a <= 1.0 {
        return x;
    }
    (1.0 + (a - 1.0).tanh()).copysign(x)
}

#[derive(Clone, Copy)]
pub struct Rng {
    state: u32,
}

impl Default for Rng {
    fn default() -> Self {
        Rng { state: 0x9E37_79B9 }
    }
}

impl Rng {
    pub fn next_u32(&mut self) -> u32 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 17;
        self.state ^= self.state << 5;
        self.state
    }
    pub fn uniform(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * (1.0 / 16_777_216.0)
    }
    pub fn bipolar(&mut self) -> f32 {
        self.uniform() * 2.0 - 1.0
    }
}

/// Power-of-two circular delay line. `read(0)` returns the most recently written sample.
/// Every index is masked, so no delay value can read outside the buffer.
#[derive(Default)]
pub struct DelayLine {
    buf: Vec<f32>,
    mask: u32,
    w: u32,
}

impl DelayLine {
    /// Allocates; not real-time safe.
    pub fn allocate(&mut self, max_delay: u32) {
        let n = next_pow2(max_delay + 4);
        self.buf = vec![0.0; n as usize];
        self.mask = n - 1;
        self.w = 0;
    }

    pub fn clear(&mut self) {
        self.buf.fill(0.0);
    }

    #[inline]
    pub fn write(&mut self, x: f32) {
        self.buf[self.w as usize] = x;
        self.w = (self.w + 1) & self.mask;
    }

    #[inline]
    pub fn read_int(&self, d: u32) -> f32 {
        self.buf[(self.w.wrapping_sub(1).wrapping_sub(d) & self.mask) as usize]
    }

    #[inline]
    pub fn read(&self, d: f32) -> f32 {
        let i = d as u32;
        let f = d - i as f32;
        let a = self.buf[(self.w.wrapping_sub(1).wrapping_sub(i) & self.mask) as usize];
        let b = self.buf[(self.w.wrapping_sub(2).wrapping_sub(i) & self.mask) as usize];
        a + (b - a) * f
    }
}

/// Schroeder allpass diffuser.
pub struct Allpass {
    pub d: DelayLine,
    len: u32,
    pub g: f32,
}

impl Default for Allpass {
    fn default() -> Self {
        Allpass { d: DelayLine::default(), len: 1, g: 0.5 }
    }
}

impl Allpass {
    pub fn allocate(&mut self, length: u32) {
        self.len = length.max(1);
        self.d.allocate(self.len);
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let delayed = self.d.read_int(self.len - 1);
        let v = x + self.g * delayed;
        self.d.write(v);
        delayed - self.g * v
    }
}

/// Cytomic/TPT state-variable filter.
#[derive(Clone, Copy)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
}

impl Default for Svf {
    fn default() -> Self {
        Svf { ic1: 0.0, ic2: 0.0, a1: 1.0, a2: 0.0, a3: 0.0, k: 1.0 }
    }
}

impl Svf {
    pub fn set(&mut self, cutoff: f32, q: f32, sample_rate: f32) {
        let g = (PI * cutoff / sample_rate).tan();
        self.k = 1.0 / q;
        self.a1 = 1.0 / (1.0 + g * (g + self.k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }

    pub fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }

    /// Returns (low-pass, high-pass).
    #[inline]
    pub fn process(&mut self, v0: f32) -> (f32, f32) {
        let v3 = v0 - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        (v2, v0 - self.k * v1 - v2)
    }
}

/// Half-precision storage for the sample pool, converted exactly like C's `_Float16`
/// (round to nearest, ties to even; subnormals and infinities preserved).
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct F16(pub u16);

impl F16 {
    pub fn from_f32(x: f32) -> F16 {
        let bits = x.to_bits();
        let sign = ((bits >> 16) & 0x8000) as u16;
        let exp = ((bits >> 23) & 0xff) as i32;
        let mant = bits & 0x007f_ffff;
        if exp == 0xff {
            // Inf or NaN (keep NaN quiet and non-zero).
            let m = if mant != 0 { 0x0200 | (mant >> 13) as u16 } else { 0 };
            return F16(sign | 0x7c00 | m);
        }
        let e = exp - 127 + 15;
        if e >= 0x1f {
            return F16(sign | 0x7c00);
        }
        if e <= 0 {
            // Subnormal (or zero) in half precision.
            if e < -10 {
                return F16(sign);
            }
            let m = mant | 0x0080_0000;
            let shift = (14 - e) as u32;
            let half = m >> shift;
            let rem = m & ((1u32 << shift) - 1);
            let halfway = 1u32 << (shift - 1);
            let round = rem > halfway || (rem == halfway && (half & 1) != 0);
            return F16(sign | (half + round as u32) as u16);
        }
        let half = ((e as u32) << 10) | (mant >> 13);
        let rem = mant & 0x1fff;
        let round = rem > 0x1000 || (rem == 0x1000 && (half & 1) != 0);
        // A carry out of the mantissa correctly bumps the exponent (up to infinity).
        F16(sign | (half + round as u32) as u16)
    }

    #[inline]
    pub fn to_f32(self) -> f32 {
        let h = self.0 as u32;
        let sign = (h & 0x8000) << 16;
        let exp = (h >> 10) & 0x1f;
        let mant = h & 0x03ff;
        let bits = if exp == 0 {
            if mant == 0 {
                sign
            } else {
                // Subnormal: normalise.
                let mut e = 127 - 15 + 1;
                let mut m = mant;
                while m & 0x0400 == 0 {
                    m <<= 1;
                    e -= 1;
                }
                sign | ((e as u32) << 23) | ((m & 0x03ff) << 13)
            }
        } else if exp == 0x1f {
            sign | 0x7f80_0000 | (mant << 13)
        } else {
            sign | ((exp + 127 - 15) << 23) | (mant << 13)
        };
        f32::from_bits(bits)
    }
}
