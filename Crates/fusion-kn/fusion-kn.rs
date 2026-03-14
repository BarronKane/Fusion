#![no_std]
//! Kernel-facing Fusion blueprint crate.
//!
//! `fusion-kn` starts as a blueprint rather than a finished kernel subsystem. Its first job
//! is to make the future kernel-facing boundary explicit:
//! - a Cargo-visible crate for policy, metadata, and evidence-planning vocabulary
//! - a Rust-for-Linux out-of-tree build seam via `Kbuild` and `Makefile`
//! - a place to record unsafe-boundary, panic, allocation, and initialization discipline
//!   before the kernel integration grows teeth
//!
//! This crate does **not** claim DO-178C compliance today. What it does is establish a
//! structure that can support a future compliance story without smuggling those decisions
//! into random module code later.

#[cfg(feature = "contract")]
#[path = "blueprint/blueprint.rs"]
/// High-level kernel blueprint records composed from metadata and evidence.
pub mod blueprint;
#[cfg(feature = "client")]
#[path = "client/client.rs"]
/// No-alloc client helpers for the mediated Fusion kernel boundary.
pub mod client;
#[cfg(feature = "contract")]
#[path = "contract/contract.rs"]
/// Strict kernel-boundary contract vocabulary.
pub mod contract;
#[cfg(feature = "contract")]
#[path = "evidence/evidence.rs"]
/// Evidence-planning vocabulary for future assurance work.
pub mod evidence;
#[cfg(feature = "contract")]
#[path = "module/module.rs"]
/// Kernel integration model and module metadata vocabulary.
pub mod module;

#[cfg(feature = "contract")]
pub use blueprint::*;
#[cfg(feature = "client")]
pub use client::*;
#[cfg(feature = "contract")]
pub use contract::*;
#[cfg(feature = "contract")]
pub use evidence::*;
#[cfg(feature = "contract")]
pub use module::*;

#[cfg(all(test, feature = "contract"))]
mod tests {
    use super::*;

    extern crate std;

    #[test]
    fn blueprint_starts_in_blueprint_phase() {
        assert_eq!(FUSION_KN_BLUEPRINT.phase, KernelBlueprintPhase::Blueprint);
        assert_eq!(
            FUSION_KN_BLUEPRINT.integration,
            KernelIntegrationModel::LinuxOutOfTreeModule
        );
        assert_eq!(
            FUSION_KN_BLUEPRINT.boundary.service_class,
            KernelServiceClass::Foundation
        );
    }

    #[test]
    fn blueprint_requires_explicit_unsafe_ledger() {
        assert_eq!(
            FUSION_KN_BLUEPRINT.boundary.unsafe_boundary_policy,
            KernelUnsafeBoundaryPolicy::ExplicitLedgerRequired
        );
        assert!(!FUSION_KN_BLUEPRINT.evidence.is_empty());
        assert!(FUSION_KN_BLUEPRINT.boundary.validate().is_ok());
        assert!(FUSION_KN_BLUEPRINT.boundary.user_surfaces.is_empty());
    }
}
