//! [`crate::world::section_decode`]'s loops under SSSE3, AVX2 and AVX-512
//! VBMI: one `target_feature` body each, the whole loop inside, so every
//! byte op inlines to vector instructions.

use crate::math::x86::bytes::{Avx2Bytes, Ssse3Bytes, VbmiBytes};
use crate::world::section::BLOCKS_PER_SECTION;
use crate::world::section_decode::{ROWS, decode_with, flag_rows_with};

miraculix::ssse3_fn! {
    pub fn decode_ssse3(
        b: Ssse3Bytes,
        bits: u8,
        bytes: &[u8],
        palette: &[u16],
        out: &mut [u16; BLOCKS_PER_SECTION],
    ) {
        decode_with(b, bits, bytes, palette, out)
    }
}

miraculix::ssse3_fn! {
    pub fn flag_rows_ssse3(
        b: Ssse3Bytes,
        bits: u8,
        bytes: &[u8],
        flags: &[u8; 256],
        len: usize,
        out: &mut [[u16; ROWS]],
    ) {
        flag_rows_with(b, bits, bytes, flags, len, out)
    }
}

miraculix::avx2_fn! {
    pub fn decode_avx2(
        b: Avx2Bytes,
        bits: u8,
        bytes: &[u8],
        palette: &[u16],
        out: &mut [u16; BLOCKS_PER_SECTION],
    ) {
        decode_with(b, bits, bytes, palette, out)
    }
}

miraculix::avx2_fn! {
    pub fn flag_rows_avx2(
        b: Avx2Bytes,
        bits: u8,
        bytes: &[u8],
        flags: &[u8; 256],
        len: usize,
        out: &mut [[u16; ROWS]],
    ) {
        flag_rows_with(b, bits, bytes, flags, len, out)
    }
}

miraculix::avx512_v4_vbmi_fn! {
    pub fn decode_vbmi(
        b: VbmiBytes,
        bits: u8,
        bytes: &[u8],
        palette: &[u16],
        out: &mut [u16; BLOCKS_PER_SECTION],
    ) {
        decode_with(b, bits, bytes, palette, out)
    }
}

miraculix::avx512_v4_vbmi_fn! {
    pub fn flag_rows_vbmi(
        b: VbmiBytes,
        bits: u8,
        bytes: &[u8],
        flags: &[u8; 256],
        len: usize,
        out: &mut [[u16; ROWS]],
    ) {
        flag_rows_with(b, bits, bytes, flags, len, out)
    }
}
