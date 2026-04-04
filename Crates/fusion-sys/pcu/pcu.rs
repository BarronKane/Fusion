//! fusion-sys PCU composition and runtime glue.
//!
//! `fusion-pcu` now owns the semantic coprocessor contract, IR, planning, preparation, and
//! generic dispatch surface. `fusion-sys::pcu` keeps only the honest system-composition lanes:
//! - local/runtime service glue such as the ingestor
//! - SoC-specific system wrappers such as Cortex-M PIO composition
//! - re-exports of the canonical `fusion-pcu` semantic surface for higher layers

mod ingestor;

#[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
pub mod cortex_m;

pub use fusion_pcu::*;
pub use ingestor::*;
