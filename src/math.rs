//! Vanilla `Mth` and `SectionPos` helpers: the circle functions and

pub mod angle;

pub use std::f32::consts::FRAC_1_SQRT_2;
pub use std::f32::consts::SQRT_2;

pub use angle::{DEG_TO_RAD, RAD_TO_DEG, cos, sin, sin_cos_deg as sin_cos_degrees, wrap_degrees};

/// Blocks along each side of a section (`SectionPos.SECTION_SIZE`).
pub const SECTION_SIZE: i32 = 16;

/// `SectionPos.blockToSectionCoord`: which section (or chunk) a block
#[inline]
pub const fn section_coord(block: i32) -> i32 {
    block >> 4
}

/// `SectionPos.sectionRelative`: a block coordinate within its section,
#[inline]
pub const fn section_relative(block: i32) -> i32 {
    block & 15
}

/// `SectionPos.sectionToBlockCoord`: a section's lowest block coordinate.
#[inline]
pub const fn section_to_block(section: i32) -> i32 {
    section << 4
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_and_splits_coordinates() {
        assert_eq!(section_coord(-1), -1);
        assert_eq!(section_relative(-1), 15);
        assert_eq!(section_coord(31), 1);
        assert_eq!(section_to_block(-2), -32);
    }
}

/// Hashes chunk coordinates with one rotate, xor and multiply per `i32`
#[derive(Default, Clone, Copy)]
pub struct ChunkHasher(u64);

impl ChunkHasher {
    const K: u64 = 0xf135_7aea_2e62_a9c5;

    #[inline]
    fn add(&mut self, value: u64) {
        self.0 = (self.0 ^ value).wrapping_mul(Self::K);
    }
}

impl std::hash::Hasher for ChunkHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0.rotate_left(26)
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.add(u64::from(byte));
        }
    }

    #[inline]
    fn write_i32(&mut self, n: i32) {
        self.add(u64::from(n as u32));
    }
}

pub type ChunkHash = std::hash::BuildHasherDefault<ChunkHasher>;
/// A map keyed by chunk `(x, z)`.
pub type ChunkMap<V> = std::collections::HashMap<(i32, i32), V, ChunkHash>;
/// A set of chunk `(x, z)`.
pub type ChunkSet = std::collections::HashSet<(i32, i32), ChunkHash>;
/// A map keyed by section `(x, y, z)`, `y` counted from the world's bottom.
pub type SectionMap<V> = std::collections::HashMap<(i32, i32, i32), V, ChunkHash>;
/// A set of sections `(x, y, z)`.
pub type SectionSet = std::collections::HashSet<(i32, i32, i32), ChunkHash>;
