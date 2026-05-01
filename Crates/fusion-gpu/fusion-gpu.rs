//! Canonical Fusion GPU composition crate.
//!
//! `fusion-gpu` sits above `fusion-pcu` and owns graphics-oriented composition law:
//! - framebuffer vocabulary
//! - pipeline/resource composition
//! - future lowering from graphics work into raw PCU instruction kernels
//!
//! It intentionally does not own:
//! - platform presentation/windowing
//! - display connector/scanout drivers
//! - backend-specific lowering glue
//! - memory-provider policy

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(test)]
extern crate std;

pub mod core;
pub mod framebuffer;
pub mod pipeline;
pub mod submission;
pub mod work;

pub use core::*;
pub use framebuffer::*;
pub use pipeline::*;
pub use submission::*;
pub use work::*;
