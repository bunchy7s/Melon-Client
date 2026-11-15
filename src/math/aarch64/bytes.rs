//! [`Bytes`] on AArch64 Neon: 16 lanes, `tbl` over 64 entries at a time.
//! A table of 256 is four chunks of 64: the first `tbl` zeroes the lanes
//! past it, each `tbx` after fills only the lanes whose index, less the
//! chunk's start, falls inside its chunk, and leaves the others as they
//! were. Neon is every AArch64's, so the kernels need no `target_feature`
//! body; the lanes are in memory order on little-endian AArch64 only.

use core::arch::aarch64::*;

use miraculix::aarch64::FeatureSet;
use miraculix::aarch64::ops::neon::neon::Neon;

use crate::math::bytes::{Bytes, chunks};

/// Each byte four times: lane `i` reads byte `i / 4` (two-bit fields).
const SPREAD_2: [u8; 16] = [0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3];
/// Each byte eight times (one-bit fields).
const SPREAD_1: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1];
/// A two-bit field's low and high bit, lane `i`'s at `2 * (i % 4)`.
const LOW_2: [u8; 16] = [1, 4, 16, 64, 1, 4, 16, 64, 1, 4, 16, 64, 1, 4, 16, 64];
const HIGH_2: [u8; 16] = [2, 8, 32, 128, 2, 8, 32, 128, 2, 8, 32, 128, 2, 8, 32, 128];
/// Lane `i`'s bit, `i % 8`: also each lane's weight in a mask.
const BIT_1: [u8; 16] = [1, 2, 4, 8, 16, 32, 64, 128, 1, 2, 4, 8, 16, 32, 64, 128];

/// 16 lanes on Neon.
#[derive(Clone, Copy)]
pub struct NeonBytes {
    _neon: Neon,
}

impl NeonBytes {
    pub fn new(features: FeatureSet) -> Option<Self> {
        Some(Self {
            _neon: Neon::from_features(features)?,
        })
    }
}

/// 64-entry chunks, as many as the table needs.
pub struct Chunks {
    chunks: [uint8x16x4_t; 4],
    count: usize,
    /// All entries in the first 16: one `tbl` of one register.
    small: bool,
}

#[inline(always)]
fn load(bytes: &[u8]) -> uint8x16_t {
    assert!(bytes.len() >= 16);
    // SAFETY: 16 bytes in bounds; Neon is every AArch64's.
    unsafe { vld1q_u8(bytes.as_ptr()) }
}

// SAFETY (every `unsafe` block below): a `NeonBytes` exists only when
// `miraculix` saw Neon; loads and stores stay inside the slices, whose
// lengths are asserted.
impl Bytes for NeonBytes {
    const LEN: usize = 16;
    type V = uint8x16_t;
    type Table = Chunks;

    #[inline(always)]
    fn table(self, entries: &[u8; 256], len: usize) -> Chunks {
        // SAFETY: 64 bytes in bounds for each chunk.
        let at = |c: usize| unsafe { vld1q_u8_x4(entries[c * 64..c * 64 + 64].as_ptr()) };
        Chunks {
            chunks: [at(0), at(1), at(2), at(3)],
            count: chunks(len, 64),
            small: len <= 16,
        }
    }

    #[inline(always)]
    fn unpack<const BITS: usize>(self, packed: &[u8]) -> uint8x16_t {
        unsafe {
            match BITS {
                8 => load(packed),
                4 => {
                    assert!(packed.len() >= 8);
                    let x = vld1_u8(packed.as_ptr());
                    let (low, high) = (vand_u8(x, vdup_n_u8(0x0F)), vshr_n_u8::<4>(x));
                    vcombine_u8(vzip1_u8(low, high), vzip2_u8(low, high))
                }
                2 => {
                    let word = u32::from_le_bytes(packed[..4].try_into().unwrap());
                    let x = vreinterpretq_u8_u32(vdupq_n_u32(word));
                    let spread = vqtbl1q_u8(x, load(&SPREAD_2));
                    let low = vandq_u8(vtstq_u8(spread, load(&LOW_2)), vdupq_n_u8(1));
                    let high = vandq_u8(vtstq_u8(spread, load(&HIGH_2)), vdupq_n_u8(2));
                    vorrq_u8(low, high)
                }
                _ => {
                    let half = u16::from_le_bytes(packed[..2].try_into().unwrap());
                    let x = vreinterpretq_u8_u16(vdupq_n_u16(half));
                    let spread = vqtbl1q_u8(x, load(&SPREAD_1));
                    vandq_u8(vtstq_u8(spread, load(&BIT_1)), vdupq_n_u8(1))
                }
            }
        }
    }

    #[inline(always)]
    fn lookup(self, table: &Chunks, index: uint8x16_t) -> uint8x16_t {
        unsafe {
            if table.small {
                return vqtbl1q_u8(table.chunks[0].0, index);
            }
            let mut out = vqtbl4q_u8(table.chunks[0], index);
            for (c, chunk) in table.chunks[..table.count].iter().enumerate().skip(1) {
                out = vqtbx4q_u8(out, *chunk, vsubq_u8(index, vdupq_n_u8(64 * c as u8)));
            }
            out
        }
    }

    #[inline(always)]
    fn store_words(self, lo: uint8x16_t, hi: uint8x16_t, out: &mut [u16]) {
        assert!(out.len() >= 16);
        unsafe {
            let p = out.as_mut_ptr().cast::<u8>();
            vst1q_u8(p, vzip1q_u8(lo, hi));
            vst1q_u8(p.add(16), vzip2q_u8(lo, hi));
        }
    }

    #[inline(always)]
    fn bit_mask(self, v: uint8x16_t, bit: u32) -> u64 {
        // No `movemask`: weigh each set lane by its bit and add the halves.
        unsafe {
            let set = vtstq_u8(v, vdupq_n_u8(1 << bit));
            let weighed = vandq_u8(set, load(&BIT_1));
            let low = vaddv_u8(vget_low_u8(weighed));
            let high = vaddv_u8(vget_high_u8(weighed));
            u64::from(low) | u64::from(high) << 8
        }
    }
}
