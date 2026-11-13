//! Post effect chains (`assets/minecraft/post_effect/<id>.json`), run the

pub mod config;
pub(in crate::renderer) mod gpu;
pub mod load;
mod record;
pub use gpu::ChainSource;
pub mod shader;
pub mod uniform;
