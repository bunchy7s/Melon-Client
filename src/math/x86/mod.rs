//! The x86_64 halves of the math kernels: [`lanes`] (SSE2, AVX, AVX-512F)
//! and each kernel's `target_feature` bodies, one file per kernel.

pub mod lanes;
pub mod sin_cos;
