//! Java `java.util.Random` compatible generator and the seed rule built on it.
//!
//! Source references:
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/GraphConfigurator.java
//! - OpenJDK `java.util.Random` seed scrambling and `nextInt(int)` semantics.

const MULTIPLIER: u64 = 0x5DEECE66D;
const ADDEND: u64 = 0xB;
const MASK: u64 = (1u64 << 48) - 1;

/// A graph asked for ELK's time-based seed.
///
/// ELK reads `randomSeed = 0` as "seed from the clock". The same graph must give the same layout
/// on every run, so this crate does not read the clock. It asks for a nonzero seed instead.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RandomSeedError {
    /// The graph with this id has `random_seed` set to 0.
    #[error(
        "ELK graph `{graph_id}` has randomSeed=0, which ELK reads as a time-based seed; set a nonzero seed so the layout stays deterministic"
    )]
    ZeroSeed {
        /// Id of the graph whose seed is 0.
        graph_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaRandom {
    seed: u64,
}

impl JavaRandom {
    pub fn new(seed: i64) -> Self {
        Self {
            seed: ((seed as u64) ^ MULTIPLIER) & MASK,
        }
    }

    pub fn next_int(&mut self, bound: usize) -> Option<usize> {
        let bound = i32::try_from(bound).ok()?;
        if bound <= 0 {
            return None;
        }

        if (bound & -bound) == bound {
            let next = i64::from(self.next_bits(31));
            return Some(((i64::from(bound) * next) >> 31) as usize);
        }

        loop {
            let bits = self.next_bits(31);
            let value = bits % bound;
            if bits.wrapping_sub(value).wrapping_add(bound - 1) >= 0 {
                return Some(value as usize);
            }
        }
    }

    pub fn next_bool(&mut self) -> bool {
        self.next_bits(1) != 0
    }

    pub fn next_float(&mut self) -> f32 {
        self.next_bits(24) as f32 / (1u32 << 24) as f32
    }

    pub fn next_double(&mut self) -> f64 {
        let high = i64::from(self.next_bits(26));
        let low = i64::from(self.next_bits(27));
        ((high << 27) + low) as f64 / (1u64 << 53) as f64
    }

    pub fn next_long(&mut self) -> i64 {
        let high = i64::from(self.next_bits(32));
        let low = i64::from(self.next_bits(32));
        (high << 32).wrapping_add(low)
    }

    pub fn set_seed(&mut self, seed: i64) {
        self.seed = ((seed as u64) ^ MULTIPLIER) & MASK;
    }

    fn next_bits(&mut self, bits: u32) -> i32 {
        self.seed = self.seed.wrapping_mul(MULTIPLIER).wrapping_add(ADDEND) & MASK;
        (self.seed >> (48 - bits)) as i32
    }
}

impl Default for JavaRandom {
    fn default() -> Self {
        Self::new(1)
    }
}

#[cfg(test)]
mod tests;
