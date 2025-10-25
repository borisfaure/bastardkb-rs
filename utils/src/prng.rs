//! Pseudo-random number generator
//!
//! This module provides a pseudo-random number generator (PRNG) based on the
//! Xorshift32 algorithm by George Marsaglia.
//!
//! See: https://en.wikipedia.org/wiki/Xorshift

/// Xorshift32 PRNG
pub struct XorShift32 {
    state: u32,
}

impl XorShift32 {
    /// Create a new XorShift32 PRNG
    pub fn new(seed: u32) -> Self {
        Self { state: seed }
    }

    /// Seed the PRNG
    pub fn seed(&mut self, seed: u32) {
        self.state = seed;
    }

    /// Get the current state
    pub fn get_state(&self) -> u32 {
        self.state
    }

    /// Get the next random number
    pub fn random(&mut self) -> u32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        x
    }
}
