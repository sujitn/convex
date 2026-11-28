//! The simulators' seeded draws, so one seed gives one run.

use std::time::Duration;

/// A seeded generator (SplitMix64): the same seed draws the same sequence.
pub struct Draws(pub u64);

impl Draws {
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`.
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in `0..n`.
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    /// Uniform between `low` and `high`.
    pub fn between(&mut self, low: Duration, high: Duration) -> Duration {
        low + (high - low).mul_f64(self.unit())
    }

    /// One of `choices`, by weight.
    pub fn weighted<T: Copy>(&mut self, choices: &[(T, u32)]) -> T {
        let total: u32 = choices.iter().map(|(_, weight)| weight).sum();
        let mut pick = (self.next_u64() % u64::from(total)) as u32;
        for (choice, weight) in choices {
            if pick < *weight {
                return *choice;
            }
            pick -= weight;
        }
        choices[choices.len() - 1].0
    }

    /// Standard normal, by Box-Muller.
    pub fn normal(&mut self) -> f64 {
        let radius = (-2.0 * (1.0 - self.unit()).ln()).sqrt();
        radius * (std::f64::consts::TAU * self.unit()).cos()
    }
}
