/// A deterministic pseudo-random number generator based on PCG-XSH-RR.
///
/// Uses PCG state transition constants and the XSH-RR output permutation
/// so that given the same seed, the sequence is stable across platforms and
/// runs. This is critical for simulation determinism in bot AI and any game
/// logic that must replay identically.
///
/// The generator is **not** cryptographically secure.

#[derive(Debug, Clone)]
pub struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    const PCG_MULTIPLIER: u64 = 6364136223846793005;
    const PCG_INCREMENT: u64 = 1442695040888963407;

    /// Create a new generator from the given seed.
    pub fn new(seed: u64) -> Self {
        // PCG recommended seeding sequence to avoid weak initial output and
        // ensure adjacent seeds do not collapse to identical streams.
        let mut rng = Self { state: 0 };
        rng.state = rng.state.wrapping_add(Self::PCG_INCREMENT);
        let _ = rng.next_u32();
        rng.state = rng.state.wrapping_add(seed);
        let _ = rng.next_u32();
        rng
    }

    /// Return the next raw 32-bit value and advance the state.
    #[inline]
    fn next_u32(&mut self) -> u32 {
        let old_state = self.state;
        self.state = old_state
            .wrapping_mul(Self::PCG_MULTIPLIER)
            .wrapping_add(Self::PCG_INCREMENT);
        let xorshifted = (((old_state >> 18) ^ old_state) >> 27) as u32;
        let rot = (old_state >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// Return the next raw 64-bit value and advance the state.
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        ((self.next_u32() as u64) << 32) | (self.next_u32() as u64)
    }

    /// Return a uniformly distributed f32 in [0, 1).
    #[inline]
    pub fn next_f32(&mut self) -> f32 {
        // Use the upper 24 bits for full mantissa precision of f32.
        (self.next_u32() >> 8) as f32 / ((1u32 << 24) as f32)
    }

    /// Return a uniformly distributed f32 in [low, high).
    #[inline]
    pub fn gen_range_f32(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.next_f32()
    }

    /// Return a uniformly distributed i32 in [low, high) (exclusive upper bound).
    #[inline]
    pub fn gen_range_i32(&mut self, low: i32, high: i32) -> i32 {
        if high <= low {
            return low;
        }
        let range = (high - low) as u64;
        low + (self.next_u64() % range) as i32
    }

    /// Return a uniformly distributed u8 in [low, high) (exclusive upper bound).
    #[inline]
    pub fn gen_range_u8(&mut self, low: u8, high: u8) -> u8 {
        if high <= low {
            return low;
        }
        let range = (high - low) as u64;
        low + (self.next_u64() % range) as u8
    }

    /// Return `true` with probability `p` (0.0 = never, 1.0 = always).
    #[inline]
    pub fn gen_bool(&mut self, p: f64) -> bool {
        if p <= 0.0 {
            return false;
        }
        if p >= 1.0 {
            return true;
        }
        (self.next_f32() as f64) < p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_sequence() {
        let mut a = DeterministicRng::new(42);
        let mut b = DeterministicRng::new(42);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_seeds_differ() {
        let mut a = DeterministicRng::new(1);
        let mut b = DeterministicRng::new(2);
        // At least one of the first 10 values should differ.
        let differs = (0..10).any(|_| a.next_u64() != b.next_u64());
        assert!(differs);
    }

    #[test]
    fn f32_in_range() {
        let mut rng = DeterministicRng::new(123);
        for _ in 0..10_000 {
            let v = rng.next_f32();
            assert!((0.0..1.0).contains(&v), "out of range: {}", v);
        }
    }

    #[test]
    fn gen_range_f32_bounds() {
        let mut rng = DeterministicRng::new(456);
        for _ in 0..10_000 {
            let v = rng.gen_range_f32(-5.0, 5.0);
            assert!((-5.0..5.0).contains(&v), "out of range: {}", v);
        }
    }

    #[test]
    fn gen_range_i32_bounds() {
        let mut rng = DeterministicRng::new(789);
        for _ in 0..10_000 {
            let v = rng.gen_range_i32(0, 100);
            assert!((0..100).contains(&v), "out of range: {}", v);
        }
    }

    #[test]
    fn gen_range_i32_invalid_bounds_returns_low() {
        let mut rng = DeterministicRng::new(7);
        assert_eq!(rng.gen_range_i32(5, 5), 5);
        assert_eq!(rng.gen_range_i32(10, 2), 10);
    }

    #[test]
    fn gen_range_u8_invalid_bounds_returns_low() {
        let mut rng = DeterministicRng::new(9);
        assert_eq!(rng.gen_range_u8(4, 4), 4);
        assert_eq!(rng.gen_range_u8(8, 1), 8);
    }

    #[test]
    fn gen_bool_extremes() {
        let mut rng = DeterministicRng::new(1);
        // p=0.0 should always be false
        for _ in 0..100 {
            assert!(!rng.gen_bool(0.0));
        }
        // p=1.0 should always be true
        for _ in 0..100 {
            assert!(rng.gen_bool(1.0));
        }
    }

    #[test]
    fn seeds_zero_and_one_do_not_collapse_to_same_stream() {
        let mut zero = DeterministicRng::new(0);
        let mut one = DeterministicRng::new(1);
        let differs = (0..16).any(|_| zero.next_u64() != one.next_u64());
        assert!(differs);
    }
}
