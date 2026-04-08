//! Ambient execution-context introspection and self-scoped local syscall surface.

#[path = "local.rs"]
pub mod local;

pub use fusion_pal::sys::context::*;
