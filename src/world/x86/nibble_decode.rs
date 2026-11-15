//! [`crate::world::nibble_decode`]'s loop under SSSE3, AVX2 and AVX-512
//! VBMI: one `target_feature` body each.

use crate::math::x86::bytes::{Avx2Bytes, Ssse3Bytes, VbmiBytes};
use crate::world::chunk::LIGHT_NIBBLES;
use crate::world::nibble_decode::unpack_with;
use crate::world::section::BLOCKS_PER_SECTION;

miraculix::ssse3_fn! {
    pub fn unpack_ssse3(
        b: Ssse3Bytes,
        packed: &[u8; LIGHT_NIBBLES],
        out: &mut [u8; BLOCKS_PER_SECTION],
    ) {
        unpack_with(b, packed, out)
    }
}

miraculix::avx2_fn! {
    pub fn unpack_avx2(
        b: Avx2Bytes,
        packed: &[u8; LIGHT_NIBBLES],
        out: &mut [u8; BLOCKS_PER_SECTION],
    ) {
        unpack_with(b, packed, out)
    }
}

miraculix::avx512_v4_vbmi_fn! {
    pub fn unpack_vbmi(
        b: VbmiBytes,
        packed: &[u8; LIGHT_NIBBLES],
        out: &mut [u8; BLOCKS_PER_SECTION],
    ) {
        unpack_with(b, packed, out)
    }
}
