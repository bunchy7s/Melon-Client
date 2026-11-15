//! [`Bytes`] on x86_64: SSSE3 (16 lanes, `pshufb` lookups), AVX2 (32, two
//! 16-lane halves) and AVX-512 VBMI (64, `vpermi2b` over 128 entries at a
//! time). `miraculix` proves the features; the byte shuffles, which it has
//! no ops for, are `core::arch`'s, reached only through a tier built from
//! those tokens.
//!
//! A table of 256 entries is 16 chunks of 16 for `pshufb`: chunk `c` takes
//! the lanes whose index, less `16 * c`, is below 16. Saturating `+ 0x70`
//! keeps those under `0x80` with their low four bits, and pushes every
//! other lane (wrapped below zero or past 15) to `0x80` and up, which
//! `pshufb` turns into zero.

use core::arch::x86_64::*;

use miraculix::x86::FeatureSet;
use miraculix::x86::ops::avx::avx2::Avx2;
use miraculix::x86::ops::avx512::avx512bw::Avx512Bw;
use miraculix::x86::ops::avx512::avx512f::Avx512f;
use miraculix::x86::ops::avx512::avx512vbmi::Avx512Vbmi;
use miraculix::x86::ops::sse::sse2::Sse2;
use miraculix::x86::ops::sse::ssse3::Ssse3;

use crate::math::bytes::{Bytes, chunks};

/// Each byte four times: lane `i` reads byte `i / 4` (two-bit fields).
const SPREAD_2: [u8; 16] = [0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3];
/// Each byte eight times (one-bit fields).
const SPREAD_1: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1];
/// A two-bit field's low and high bit, lane `i`'s at `2 * (i % 4)`.
const LOW_2: [u8; 16] = [1, 4, 16, 64, 1, 4, 16, 64, 1, 4, 16, 64, 1, 4, 16, 64];
const HIGH_2: [u8; 16] = [2, 8, 32, 128, 2, 8, 32, 128, 2, 8, 32, 128, 2, 8, 32, 128];
/// Lane `i`'s bit, `i % 8`.
const BIT_1: [u8; 16] = [1, 2, 4, 8, 16, 32, 64, 128, 1, 2, 4, 8, 16, 32, 64, 128];

/// `pattern` (16 bytes) once a 128-bit lane, each lane's bytes moved on by
/// `step` from the last's: the in-lane shuffles of the wide tiers.
const fn lanes<const N: usize>(pattern: [u8; 16], step: u8) -> [u8; N] {
    let mut out = [0u8; N];
    let mut i = 0;
    while i < N {
        out[i] = pattern[i % 16] + step * (i / 16) as u8;
        i += 1;
    }
    out
}

/// 16 lanes on SSSE3.
#[derive(Clone, Copy)]
pub struct Ssse3Bytes {
    _sse2: Sse2,
    _ssse3: Ssse3,
}

impl Ssse3Bytes {
    pub fn new(features: FeatureSet) -> Option<Self> {
        Some(Self {
            _sse2: Sse2::from_features(features)?,
            _ssse3: Ssse3::from_features(features)?,
        })
    }
}

/// 16-entry chunks, as many as the table needs.
pub struct Chunks<V> {
    chunks: [V; 16],
    count: usize,
}

#[inline(always)]
fn load_128(bytes: &[u8]) -> __m128i {
    assert!(bytes.len() >= 16);
    // SAFETY: 16 bytes in bounds, unaligned load.
    unsafe { _mm_loadu_si128(bytes.as_ptr().cast()) }
}

// SAFETY (every `unsafe` block below): an `Ssse3Bytes` exists only when
// `miraculix` saw SSE2 and SSSE3, and the kernels inline these methods into
// a `target_feature` body enabling them; loads and stores stay inside the
// slices, whose lengths are asserted.
impl Bytes for Ssse3Bytes {
    const LEN: usize = 16;
    type V = __m128i;
    type Table = Chunks<__m128i>;

    #[inline(always)]
    fn table(self, entries: &[u8; 256], len: usize) -> Self::Table {
        let count = chunks(len, 16);
        let mut chunks = [unsafe { _mm_setzero_si128() }; 16];
        for (c, chunk) in chunks.iter_mut().enumerate().take(count) {
            *chunk = load_128(&entries[c * 16..]);
        }
        Chunks { chunks, count }
    }

    #[inline(always)]
    fn unpack<const BITS: usize>(self, packed: &[u8]) -> __m128i {
        unsafe {
            match BITS {
                8 => load_128(packed),
                4 => {
                    let x = _mm_cvtsi64_si128(i64::from_le_bytes(packed[..8].try_into().unwrap()));
                    let nibble = _mm_set1_epi8(0x0F);
                    let low = _mm_and_si128(x, nibble);
                    let high = _mm_and_si128(_mm_srli_epi16(x, 4), nibble);
                    _mm_unpacklo_epi8(low, high)
                }
                2 => {
                    let x = _mm_cvtsi32_si128(i32::from_le_bytes(packed[..4].try_into().unwrap()));
                    let spread = _mm_shuffle_epi8(x, load_128(&SPREAD_2));
                    let (low, high) = (load_128(&LOW_2), load_128(&HIGH_2));
                    let low = _mm_cmpeq_epi8(_mm_and_si128(spread, low), low);
                    let high = _mm_cmpeq_epi8(_mm_and_si128(spread, high), high);
                    _mm_or_si128(
                        _mm_and_si128(low, _mm_set1_epi8(1)),
                        _mm_and_si128(high, _mm_set1_epi8(2)),
                    )
                }
                _ => {
                    let x = _mm_cvtsi32_si128(i32::from(u16::from_le_bytes(
                        packed[..2].try_into().unwrap(),
                    )));
                    let spread = _mm_shuffle_epi8(x, load_128(&SPREAD_1));
                    let bit = load_128(&BIT_1);
                    let set = _mm_cmpeq_epi8(_mm_and_si128(spread, bit), bit);
                    _mm_and_si128(set, _mm_set1_epi8(1))
                }
            }
        }
    }

    #[inline(always)]
    fn lookup(self, table: &Self::Table, index: __m128i) -> __m128i {
        unsafe {
            if table.count == 1 {
                return _mm_shuffle_epi8(table.chunks[0], index);
            }
            let (bias, sixteen) = (_mm_set1_epi8(0x70), _mm_set1_epi8(16));
            let mut out = _mm_setzero_si128();
            let mut rest = index;
            for chunk in &table.chunks[..table.count] {
                let hit = _mm_shuffle_epi8(*chunk, _mm_adds_epu8(rest, bias));
                out = _mm_or_si128(out, hit);
                rest = _mm_sub_epi8(rest, sixteen);
            }
            out
        }
    }

    #[inline(always)]
    fn store_words(self, lo: __m128i, hi: __m128i, out: &mut [u16]) {
        assert!(out.len() >= 16);
        unsafe {
            let p = out.as_mut_ptr().cast::<__m128i>();
            _mm_storeu_si128(p, _mm_unpacklo_epi8(lo, hi));
            _mm_storeu_si128(p.add(1), _mm_unpackhi_epi8(lo, hi));
        }
    }

    #[inline(always)]
    fn bit_mask(self, v: __m128i, bit: u32) -> u64 {
        // The bit to the top of each byte, where `pmovmskb` reads it.
        unsafe {
            let up = _mm_sll_epi16(v, _mm_cvtsi32_si128(7 - bit as i32));
            u64::from(_mm_movemask_epi8(up) as u16)
        }
    }
}

/// 32 lanes on AVX2: two 16-lane halves, each shuffled on its own.
#[derive(Clone, Copy)]
pub struct Avx2Bytes {
    _ssse3: Ssse3Bytes,
    _avx2: Avx2,
}

impl Avx2Bytes {
    pub fn new(features: FeatureSet) -> Option<Self> {
        Some(Self {
            _ssse3: Ssse3Bytes::new(features)?,
            _avx2: Avx2::from_features(features)?,
        })
    }
}

#[inline(always)]
fn load_256(bytes: &[u8]) -> __m256i {
    assert!(bytes.len() >= 32);
    // SAFETY: 32 bytes in bounds, unaligned load.
    unsafe { _mm256_loadu_si256(bytes.as_ptr().cast()) }
}

const SPREAD_2_256: [u8; 32] = lanes(SPREAD_2, 4);
const SPREAD_1_256: [u8; 32] = lanes(SPREAD_1, 2);
const LOW_2_256: [u8; 32] = lanes(LOW_2, 0);
const HIGH_2_256: [u8; 32] = lanes(HIGH_2, 0);
const BIT_1_256: [u8; 32] = lanes(BIT_1, 0);

// SAFETY: as for `Ssse3Bytes`, with AVX2.
impl Bytes for Avx2Bytes {
    const LEN: usize = 32;
    type V = __m256i;
    type Table = Chunks<__m256i>;

    #[inline(always)]
    fn table(self, entries: &[u8; 256], len: usize) -> Self::Table {
        let count = chunks(len, 16);
        let mut chunks = [unsafe { _mm256_setzero_si256() }; 16];
        for (c, chunk) in chunks.iter_mut().enumerate().take(count) {
            *chunk = unsafe { _mm256_broadcastsi128_si256(load_128(&entries[c * 16..])) };
        }
        Chunks { chunks, count }
    }

    #[inline(always)]
    fn unpack<const BITS: usize>(self, packed: &[u8]) -> __m256i {
        unsafe {
            match BITS {
                8 => load_256(packed),
                4 => {
                    // A byte a word: its low nibble stays, its high one
                    // moves up into the word's second byte.
                    let w = _mm256_cvtepu8_epi16(load_128(packed));
                    _mm256_or_si256(
                        _mm256_and_si256(w, _mm256_set1_epi16(0x000F)),
                        _mm256_and_si256(_mm256_slli_epi16(w, 4), _mm256_set1_epi16(0x0F00)),
                    )
                }
                2 => {
                    let x = _mm256_set1_epi64x(i64::from_le_bytes(packed[..8].try_into().unwrap()));
                    let spread = _mm256_shuffle_epi8(x, load_256(&SPREAD_2_256));
                    let (low, high) = (load_256(&LOW_2_256), load_256(&HIGH_2_256));
                    let low = _mm256_cmpeq_epi8(_mm256_and_si256(spread, low), low);
                    let high = _mm256_cmpeq_epi8(_mm256_and_si256(spread, high), high);
                    _mm256_or_si256(
                        _mm256_and_si256(low, _mm256_set1_epi8(1)),
                        _mm256_and_si256(high, _mm256_set1_epi8(2)),
                    )
                }
                _ => {
                    let x = _mm256_set1_epi32(i32::from_le_bytes(packed[..4].try_into().unwrap()));
                    let spread = _mm256_shuffle_epi8(x, load_256(&SPREAD_1_256));
                    let bit = load_256(&BIT_1_256);
                    let set = _mm256_cmpeq_epi8(_mm256_and_si256(spread, bit), bit);
                    _mm256_and_si256(set, _mm256_set1_epi8(1))
                }
            }
        }
    }

    #[inline(always)]
    fn lookup(self, table: &Self::Table, index: __m256i) -> __m256i {
        unsafe {
            if table.count == 1 {
                return _mm256_shuffle_epi8(table.chunks[0], index);
            }
            let (bias, sixteen) = (_mm256_set1_epi8(0x70), _mm256_set1_epi8(16));
            let mut out = _mm256_setzero_si256();
            let mut rest = index;
            for chunk in &table.chunks[..table.count] {
                let hit = _mm256_shuffle_epi8(*chunk, _mm256_adds_epu8(rest, bias));
                out = _mm256_or_si256(out, hit);
                rest = _mm256_sub_epi8(rest, sixteen);
            }
            out
        }
    }

    #[inline(always)]
    fn store_words(self, lo: __m256i, hi: __m256i, out: &mut [u16]) {
        assert!(out.len() >= 32);
        unsafe {
            // Interleaving works within halves: put the halves back in order.
            let (a, b) = (_mm256_unpacklo_epi8(lo, hi), _mm256_unpackhi_epi8(lo, hi));
            let p = out.as_mut_ptr().cast::<__m256i>();
            _mm256_storeu_si256(p, _mm256_permute2x128_si256(a, b, 0x20));
            _mm256_storeu_si256(p.add(1), _mm256_permute2x128_si256(a, b, 0x31));
        }
    }

    #[inline(always)]
    fn bit_mask(self, v: __m256i, bit: u32) -> u64 {
        unsafe {
            let up = _mm256_sll_epi16(v, _mm_cvtsi32_si128(7 - bit as i32));
            u64::from(_mm256_movemask_epi8(up) as u32)
        }
    }
}

/// 64 lanes on AVX-512 with VBMI: `vpermi2b` looks up 128 entries at once.
#[derive(Clone, Copy)]
pub struct VbmiBytes {
    _avx2: Avx2Bytes,
    _f: Avx512f,
    _bw: Avx512Bw,
    _vbmi: Avx512Vbmi,
}

impl VbmiBytes {
    pub fn new(features: FeatureSet) -> Option<Self> {
        Some(Self {
            _avx2: Avx2Bytes::new(features)?,
            _f: Avx512f::from_features(features)?,
            _bw: Avx512Bw::from_features(features)?,
            _vbmi: Avx512Vbmi::from_features(features)?,
        })
    }
}

/// Entries `0..128` and `128..256`, each as two 64-byte halves.
pub struct Wide {
    halves: [[__m512i; 2]; 2],
    count: usize,
}

#[inline(always)]
fn load_512(bytes: &[u8]) -> __m512i {
    assert!(bytes.len() >= 64);
    // SAFETY: 64 bytes in bounds, unaligned load.
    unsafe { _mm512_loadu_si512(bytes.as_ptr().cast()) }
}

const SPREAD_2_512: [u8; 64] = lanes(SPREAD_2, 4);
const LOW_2_512: [u8; 64] = lanes(LOW_2, 0);
const HIGH_2_512: [u8; 64] = lanes(HIGH_2, 0);

// SAFETY: as for `Ssse3Bytes`, with AVX-512F, BW and VBMI.
impl Bytes for VbmiBytes {
    const LEN: usize = 64;
    type V = __m512i;
    type Table = Wide;

    #[inline(always)]
    fn table(self, entries: &[u8; 256], len: usize) -> Wide {
        let at = |i: usize| load_512(&entries[i * 64..]);
        Wide {
            halves: [[at(0), at(1)], [at(2), at(3)]],
            count: chunks(len, 128),
        }
    }

    #[inline(always)]
    fn unpack<const BITS: usize>(self, packed: &[u8]) -> __m512i {
        unsafe {
            match BITS {
                8 => load_512(packed),
                4 => {
                    let w = _mm512_cvtepu8_epi16(_mm256_loadu_si256(packed[..32].as_ptr().cast()));
                    _mm512_or_si512(
                        _mm512_and_si512(w, _mm512_set1_epi16(0x000F)),
                        _mm512_and_si512(_mm512_slli_epi16(w, 4), _mm512_set1_epi16(0x0F00)),
                    )
                }
                2 => {
                    let x = _mm512_broadcast_i32x4(load_128(packed));
                    let spread = _mm512_shuffle_epi8(x, load_512(&SPREAD_2_512));
                    let low = _mm512_test_epi8_mask(spread, load_512(&LOW_2_512));
                    let high = _mm512_test_epi8_mask(spread, load_512(&HIGH_2_512));
                    _mm512_or_si512(
                        _mm512_maskz_mov_epi8(low, _mm512_set1_epi8(1)),
                        _mm512_maskz_mov_epi8(high, _mm512_set1_epi8(2)),
                    )
                }
                _ => {
                    // 64 one-bit fields are a mask already.
                    let bits = u64::from_le_bytes(packed[..8].try_into().unwrap());
                    _mm512_maskz_mov_epi8(bits, _mm512_set1_epi8(1))
                }
            }
        }
    }

    #[inline(always)]
    fn lookup(self, table: &Wide, index: __m512i) -> __m512i {
        unsafe {
            let [[a, b], [c, d]] = table.halves;
            let low = _mm512_permutex2var_epi8(a, index, b);
            if table.count == 1 {
                return low;
            }
            let high = _mm512_permutex2var_epi8(c, index, d);
            _mm512_mask_blend_epi8(_mm512_movepi8_mask(index), low, high)
        }
    }

    #[inline(always)]
    fn store_words(self, lo: __m512i, hi: __m512i, out: &mut [u16]) {
        assert!(out.len() >= 64);
        unsafe {
            // Interleaved quarter by quarter; quarter `k` of `a` and of `b`
            // are words `16k..16k+8` and `16k+8..16k+16`.
            let (a, b) = (_mm512_unpacklo_epi8(lo, hi), _mm512_unpackhi_epi8(lo, hi));
            let first = _mm512_set_epi64(11, 10, 3, 2, 9, 8, 1, 0);
            let second = _mm512_set_epi64(15, 14, 7, 6, 13, 12, 5, 4);
            let p = out.as_mut_ptr().cast::<__m512i>();
            _mm512_storeu_si512(p, _mm512_permutex2var_epi64(a, first, b));
            _mm512_storeu_si512(p.add(1), _mm512_permutex2var_epi64(a, second, b));
        }
    }

    #[inline(always)]
    fn bit_mask(self, v: __m512i, bit: u32) -> u64 {
        unsafe { _mm512_test_epi8_mask(v, _mm512_set1_epi8((1u8 << bit) as i8)) }
    }
}
