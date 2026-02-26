/// A deterministic pseudo-random number generator based on the PCG family.
///
/// Uses the same constants as PCG-XSH-RR (multiplier and increment from
/// Knuth's LCG / PCG paper) so that given the same seed, the sequence of
/// outputs is identical across platforms and runs.  This is critical for
/// simulation determinism in bot AI and any game logic that must replay
/// identically.
///
/// The generator is **not** cryptographically secure.

#[derive(Debug, Clone)]
pub struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    /// Create a new generator from the given seed.
    pub fn new(seed: u64) -> Self {
        // Ensure the initial state is well-mixed even for small seeds.
        let mut rng = Self { state: 0 };
        rng.state = rng.state.wrapping_add(seed | 1);
        let _ = rng.next_u64(); // Advance once to mix.
        rng
    }

    /// Return the next raw 64-bit value and advance the state.
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }

    /// Return a uniformly distributed f32 in [0, 1).
    #[inline]
    pub fn next_f32(&mut self) -> f32 {
        // Use the upper 24 bits for full mantissa precision of f32.
        (self.next_u64() >> 40) as f32 / ((1u64 << 24) as f32)
    }

    /// Return a uniformly distributed f32 in [low, high).
    #[inline]
    pub fn gen_range_f32(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.next_f32()
    }

    /// Return a uniformly distributed i32 in [low, high) (exclusive upper bound).
    #[inline]
    pub fn gen_range_i32(&mut self, low: i32, high: i32) -> i32 {
        debug_assert!(high > low, "gen_range_i32: high must be > low");
        let range = (high - low) as u64;
        low + (self.next_u64() % range) as i32
    }

    /// Return a uniformly distributed u8 in [low, high) (exclusive upper bound).
    #[inline]
    pub fn gen_range_u8(&mut self, low: u8, high: u8) -> u8 {
        debug_assert!(high > low, "gen_range_u8: high must be > low");
        let range = (high - low) as u64;
        low + (self.next_u64() % range) as u8
    }

    /// Return `true` with probability `p` (0.0 = never, 1.0 = always).
    #[inline]
    pub fn gen_bool(&mut self, p: f64) -> bool {
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
}
