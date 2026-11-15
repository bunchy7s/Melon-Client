//! [`crate::math::sin_cos`]'s loop under AVX and AVX-512F: one
//! `target_feature` body each, the whole loop inside, so every lane op
//! inlines to vector instructions. (SSE2 is every x86_64's baseline and
//! needs none.)

use super::lanes::{Avx512Lanes, AvxLanes};
use crate::math::sin_cos::run;

miraculix::avx_fn! {
    pub fn avx(lanes: AvxLanes, angles: &[f32], sin: &mut [f32], cos: &mut [f32]) {
        run(lanes, angles, sin, cos)
    }
}

miraculix::avx512f_fn! {
    pub fn avx512(lanes: Avx512Lanes, angles: &[f32], sin: &mut [f32], cos: &mut [f32]) {
        run(lanes, angles, sin, cos)
    }
}
