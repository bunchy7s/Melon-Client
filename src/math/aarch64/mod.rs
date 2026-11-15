//! The AArch64 halves of the math kernels: [`lanes`] (Neon). Neon is every
//! AArch64's baseline, so the kernels need no `target_feature` bodies.

pub mod lanes;
