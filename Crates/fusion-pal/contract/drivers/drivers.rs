//! Driver-facing contracts that still live in `fusion-pal`.
//!
//! `fusion-hal` now owns the general driver contract tree. This module remains only for PAL-local
//! or transitional driver contracts that have not been lifted yet.

#[path = "pcu/pcu.rs"]
pub mod pcu;
