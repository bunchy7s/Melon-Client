//! Which instruction set the math kernels run on, decided once.
//!
//! [`init`] probes the CPU at Melon's start (`miraculix`'s feature cache)
//! and settles the widest [`Tier`]; every kernel asks [`tier`] after that,
//! a load of a cached value. A kernel sits beside the scalar code it
//! speeds up (`sin_cos.rs` beside `angle.rs`), written once against
//! [`super::lanes::Lanes`]; its x86_64 `target_feature` bodies are in
//! `x86/`, the lanes of each architecture in `x86/` and `aarch64/`.
//!
//! x86_64 and AArch64 have kernels; other CPUs (RISC-V, PowerPC, wasm)
//! run the scalar code for now.

use std::sync::OnceLock;

/// The instruction set the kernels run on.
#[derive(Clone, Copy)]
pub enum Tier {
    /// No vector unit Melon has kernels for.
    Scalar,
    /// Every x86_64: two doubles a register.
    #[cfg(target_arch = "x86_64")]
    Sse2(super::x86::lanes::Sse2Lanes),
    #[cfg(target_arch = "x86_64")]
    Avx(super::x86::lanes::AvxLanes),
    #[cfg(target_arch = "x86_64")]
    Avx512(super::x86::lanes::Avx512Lanes),
    /// Every AArch64: two doubles a register.
    #[cfg(target_arch = "aarch64")]
    Neon(super::aarch64::lanes::NeonLanes),
}

impl Tier {
    pub fn name(self) -> &'static str {
        match self {
            Tier::Scalar => "scalar",
            #[cfg(target_arch = "x86_64")]
            Tier::Sse2(_) => "SSE2",
            #[cfg(target_arch = "x86_64")]
            Tier::Avx(_) => "AVX",
            #[cfg(target_arch = "x86_64")]
            Tier::Avx512(_) => "AVX-512F",
            #[cfg(target_arch = "aarch64")]
            Tier::Neon(_) => "Neon",
        }
    }

    /// Every tier this CPU can run, widest first, [`Tier::Scalar`] last.
    pub fn available() -> Vec<Tier> {
        let mut tiers = Vec::new();
        #[cfg(target_arch = "x86_64")]
        {
            let features = miraculix::x86::detect_features();
            tiers.extend(super::x86::lanes::Avx512Lanes::new(features).map(Tier::Avx512));
            tiers.extend(super::x86::lanes::AvxLanes::new(features).map(Tier::Avx));
            tiers.extend(super::x86::lanes::Sse2Lanes::new(features).map(Tier::Sse2));
        }
        #[cfg(target_arch = "aarch64")]
        {
            let features = miraculix::aarch64::detect_features();
            tiers.extend(super::aarch64::lanes::NeonLanes::new(features).map(Tier::Neon));
        }
        tiers.push(Tier::Scalar);
        tiers
    }
}

/// Buffers this many lanes long (zero padded) only ever take whole steps
/// on every tier: no copied, padded last one.
pub const PAD: usize = 4;

static TIER: OnceLock<Tier> = OnceLock::new();

/// At Melon's start: fills `miraculix`'s feature cache and settles the
/// tier every kernel runs on from then on.
pub fn init() -> Tier {
    #[cfg(target_arch = "x86_64")]
    miraculix::x86::warm_up();
    #[cfg(target_arch = "aarch64")]
    miraculix::aarch64::warm_up();
    tier()
}

/// The tier [`init`] settled (probed now if nothing called it, as in tests):
/// the widest, or the one `MELON_SIMD` names (`AVX`, `SSE2`, `scalar`, ...),
/// if this CPU has it.
#[inline]
pub fn tier() -> Tier {
    *TIER.get_or_init(|| {
        let available = Tier::available();
        let wanted = std::env::var("MELON_SIMD").ok();
        wanted
            .and_then(|name| {
                available
                    .iter()
                    .copied()
                    .find(|tier| tier.name().eq_ignore_ascii_case(&name))
            })
            .unwrap_or(available[0])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The baseline every x86_64 and AArch64 has is found, so its kernels
    /// run (and their tests test them) everywhere.
    #[test]
    fn the_baseline_tier_is_there() {
        let names: Vec<&str> = Tier::available().into_iter().map(Tier::name).collect();
        #[cfg(target_arch = "x86_64")]
        assert!(names.contains(&"SSE2"), "{names:?}");
        #[cfg(target_arch = "aarch64")]
        assert!(names.contains(&"Neon"), "{names:?}");
        assert_eq!(names.last(), Some(&"scalar"));
        assert_eq!(tier().name(), names[0]);
    }
}
