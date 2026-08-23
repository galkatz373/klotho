//! One deterministic RNG for the whole engine (K25).
//!
//! Seeded from `canon_hash ⊕ tick`. No OS entropy. Allocator addresses must
//! never be mixed in. xoshiro256++ is the generator; SplitMix64 expands the
//! 256-bit seed. Same `(Hash, Tick)` ⇒ same stream on every OS.

use crate::{Hash, Tick};

/// Deterministic xoshiro256++ engine.
///
/// Construct with [`Rng::seed`]. Do not `Default` — an unseeded generator
/// would be a hidden source of world state (K22).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Rng {
    s: [u64; 4],
}

impl Rng {
    /// Seed from Canon hash XOR tick, as specified by K25.
    ///
    /// The tick is written little-endian into the first eight bytes of a
    /// zero block, XOR'd with the 32-byte hash, then expanded with SplitMix64.
    /// All 32 hash bytes contribute.
    #[must_use]
    pub fn seed(canon_hash: Hash, tick: Tick) -> Self {
        let mixed = mix_canon_tick(canon_hash, tick);
        let mut sm = splitmix_seed(mixed);
        let mut s = [
            splitmix64(&mut sm),
            splitmix64(&mut sm),
            splitmix64(&mut sm),
            splitmix64(&mut sm),
        ];
        // xoshiro256 degenerates on the all-zero state.
        if s[0] | s[1] | s[2] | s[3] == 0 {
            s[0] = 0x9e3779b97f4a7c15;
        }
        Self { s }
    }

    /// Next 64 bits.
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let result = rotl(self.s[0].wrapping_add(self.s[3]), 23).wrapping_add(self.s[0]);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = rotl(self.s[3], 45);
        result
    }

    /// Next 32 bits (high half of [`Self::next_u64`]).
    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// Fill `out` with little-endian u64 chunks (last chunk truncated).
    pub fn fill_bytes(&mut self, out: &mut [u8]) {
        let mut i = 0;
        while i + 8 <= out.len() {
            let n = self.next_u64().to_le_bytes();
            out[i..i + 8].copy_from_slice(&n);
            i += 8;
        }
        if i < out.len() {
            let n = self.next_u64().to_le_bytes();
            let rest = out.len() - i;
            out[i..].copy_from_slice(&n[..rest]);
        }
    }

    /// Uniform `u32` in `0..upper` (unbiased via rejection). `upper == 0` → 0.
    pub fn gen_u32(&mut self, upper: u32) -> u32 {
        if upper <= 1 {
            return 0;
        }
        let max = upper as u64;
        // Threshold: largest multiple of `max` that fits in u64.
        let thresh = u64::MAX - (u64::MAX % max);
        loop {
            let r = self.next_u64();
            if r < thresh {
                return (r % max) as u32;
            }
        }
    }
}

fn mix_canon_tick(canon_hash: Hash, tick: Tick) -> [u8; 32] {
    let mut mixed = *canon_hash.as_bytes();
    let t = tick.0.to_le_bytes();
    for (i, b) in t.iter().enumerate() {
        mixed[i] ^= b;
    }
    mixed
}

fn splitmix_seed(mixed: [u8; 32]) -> u64 {
    let mut acc = 0u64;
    let mut i = 0;
    while i < 32 {
        let mut chunk = [0u8; 8];
        chunk.copy_from_slice(&mixed[i..i + 8]);
        acc ^= u64::from_le_bytes(chunk).rotate_left((i as u32) * 8);
        i += 8;
    }
    if acc == 0 { 0x9e3779b97f4a7c15 } else { acc }
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e3779b97f4a7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}

#[inline]
const fn rotl(x: u64, k: u32) -> u64 {
    x.rotate_left(k)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash_from_byte(b: u8) -> Hash {
        let mut bytes = [0u8; 32];
        bytes[0] = b;
        bytes[31] = b.wrapping_add(1);
        Hash(bytes)
    }

    #[test]
    fn same_seed_same_stream() {
        let h = hash_from_byte(0xab);
        let mut a = Rng::seed(h, Tick(7));
        let mut b = Rng::seed(h, Tick(7));
        let va: Vec<u64> = (0..32).map(|_| a.next_u64()).collect();
        let vb: Vec<u64> = (0..32).map(|_| b.next_u64()).collect();
        assert_eq!(va, vb);
    }

    #[test]
    fn tick_changes_stream() {
        let h = hash_from_byte(0xab);
        let mut a = Rng::seed(h, Tick(7));
        let mut b = Rng::seed(h, Tick(8));
        assert_ne!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn hash_changes_stream() {
        let mut a = Rng::seed(hash_from_byte(1), Tick(0));
        let mut b = Rng::seed(hash_from_byte(2), Tick(0));
        assert_ne!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn zero_seed_is_nonzero_stream() {
        let mut r = Rng::seed(Hash::ZERO, Tick::ZERO);
        let sample: Vec<u64> = (0..8).map(|_| r.next_u64()).collect();
        assert!(sample.iter().any(|&x| x != 0));
        assert_ne!(sample[0], sample[1]);
    }

    #[test]
    fn gen_u32_stays_in_range() {
        let mut r = Rng::seed(hash_from_byte(9), Tick(1));
        assert_eq!(r.gen_u32(0), 0);
        assert_eq!(r.gen_u32(1), 0);
        for _ in 0..256 {
            let v = r.gen_u32(10);
            assert!(v < 10);
        }
    }

    #[test]
    fn fill_bytes_round_trips_u64_le() {
        let mut r = Rng::seed(hash_from_byte(3), Tick(4));
        let mut clone = r.clone();
        let n = r.next_u64();
        let mut buf = [0u8; 8];
        clone.fill_bytes(&mut buf);
        assert_eq!(buf, n.to_le_bytes());
    }

    #[test]
    fn golden_first_draw_is_stable() {
        // Pin the mixing function. If this changes, replay hashes change.
        let mut r = Rng::seed(hash_from_byte(0x11), Tick(42));
        assert_eq!(r.next_u64(), 0x9872_44ca_43d0_5252);
    }
}
