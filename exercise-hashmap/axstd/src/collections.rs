//! Collection types.
//!
//! Mirrors the layout of Rust's `std::collections`: the sequence/map/set
//! containers from [`alloc`] plus a hash-based [`HashMap`]/[`HashSet`].
//!
//! ArceOS is `no_std`, so the standard library's `HashMap` (which relies on
//! `std`'s OS-backed `RandomState`) is not available. We provide one here on
//! top of [`hashbrown`], using a [`RandomState`] whose seed is taken from the
//! hardware monotonic/wall clock exposed by ArceOS. This release of `axhal`
//! does not expose a dedicated `random()` syscall, so the on-boot clock value
//! is the entropy source used to randomize the hasher (mitigating hash-flooding
//! by making the iteration/insertion order non-deterministic across runs).

#[doc(no_inline)]
pub use alloc::collections::*;

use core::hash::{BuildHasher, Hasher};
use core::ops::{Deref, DerefMut};

/// A hasher state that seeds an [`FxHasher`] from a per-instance seed.
///
/// The seed is derived from the ArceOS hardware clock
/// ([`arceos_api::time::ax_wall_time`]) the first time a [`HashMap`] is
/// created, so the hash function differs from run to run.
#[derive(Clone, Copy, Debug)]
pub struct RandomState {
    seed: u64,
}

impl RandomState {
    /// Create a new [`RandomState`] seeded from the hardware clock.
    pub fn new() -> Self {
        // ArceOS has no `random()` in this release; use the wall clock as the
        // entropy source and avalanche-mix it (splitmix64 finalizer).
        let nanos = arceos_api::time::ax_wall_time().as_nanos() as u64;
        let mut z = nanos.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        Self { seed: z ^ (z >> 31) }
    }
}

impl Default for RandomState {
    fn default() -> Self {
        Self::new()
    }
}

impl BuildHasher for RandomState {
    type Hasher = FxHasher;
    fn build_hasher(&self) -> FxHasher {
        FxHasher { hash: self.seed }
    }
}

/// A fast, seedable, `no_std` hasher (FxHash construction).
#[derive(Clone)]
pub struct FxHasher {
    hash: u64,
}

impl FxHasher {
    const K: u64 = 0x517c_c1b7_2722_0a95;

    #[inline]
    fn add(&mut self, i: u64) {
        self.hash = (self.hash.rotate_left(5) ^ i).wrapping_mul(Self::K);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, mut bytes: &[u8]) {
        while bytes.len() >= 8 {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(&bytes[..8]);
            self.add(u64::from_le_bytes(buf));
            bytes = &bytes[8..];
        }
        for &b in bytes {
            self.add(b as u64);
        }
    }

    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(i as u64);
    }

    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(i as u64);
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }

    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
}

/// A hash map, like Rust's `std::collections::HashMap`.
///
/// Backed by [`hashbrown`] with a clock-seeded [`RandomState`]. The public
/// surface (`new`, `insert`, `get`, `iter`, ...) is provided through `Deref`
/// to the underlying `hashbrown::HashMap`.
pub struct HashMap<K, V, S = RandomState> {
    base: hashbrown::HashMap<K, V, S>,
}

impl<K, V> HashMap<K, V, RandomState> {
    /// Creates an empty `HashMap` with a clock-seeded hasher.
    #[inline]
    pub fn new() -> Self {
        Self {
            base: hashbrown::HashMap::with_hasher(RandomState::new()),
        }
    }

    /// Creates an empty `HashMap` with at least the given capacity.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            base: hashbrown::HashMap::with_capacity_and_hasher(capacity, RandomState::new()),
        }
    }
}

impl<K, V, S> HashMap<K, V, S> {
    /// Creates an empty `HashMap` using the given hash builder.
    #[inline]
    pub fn with_hasher(hash_builder: S) -> Self {
        Self {
            base: hashbrown::HashMap::with_hasher(hash_builder),
        }
    }
}

impl<K, V> Default for HashMap<K, V, RandomState> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V, S> Deref for HashMap<K, V, S> {
    type Target = hashbrown::HashMap<K, V, S>;
    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl<K, V, S> DerefMut for HashMap<K, V, S> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.base
    }
}

impl<'a, K, V, S> IntoIterator for &'a HashMap<K, V, S> {
    type Item = (&'a K, &'a V);
    type IntoIter = hashbrown::hash_map::Iter<'a, K, V>;
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.base.iter()
    }
}

impl<K, V, S> IntoIterator for HashMap<K, V, S> {
    type Item = (K, V);
    type IntoIter = hashbrown::hash_map::IntoIter<K, V>;
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.base.into_iter()
    }
}

/// A hash set, like Rust's `std::collections::HashSet`.
pub type HashSet<T, S = RandomState> = hashbrown::HashSet<T, S>;
