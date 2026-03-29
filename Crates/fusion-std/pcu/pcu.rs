//! Public PCU front door.
//!
//! `fusion-std::pcu` stays intentionally thin:
//! - `fusion-pal` owns contract and backend truth
//! - `fusion-sys` owns direct local composition and executor/runtime helpers
//! - `fusion-std` keeps only macro sugar and the public re-export seam

pub use fusion_std_pcu_macros::PCU;
pub use fusion_sys::pcu::*;
