//! Public PCU front door.
//!
//! `fusion-std::pcu` stays intentionally thin:
//! - `fusion-pal` owns backend/provider truth
//! - `fusion-pcu` owns the semantic contract, IR, and dispatch surface
//! - `fusion-sys` owns local composition and executor/runtime glue
//! - `fusion-std` keeps only macro sugar and the public re-export seam

pub use fusion_std_pcu_macros::PCU;
pub use fusion_sys::pcu::*;
