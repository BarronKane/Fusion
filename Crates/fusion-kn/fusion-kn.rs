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

#[path = "blueprint/blueprint.rs"]
/// High-level kernel blueprint records composed from metadata and evidence.
pub mod blueprint;
#[path = "evidence/evidence.rs"]
/// Evidence-planning vocabulary for future assurance work.
pub mod evidence;
#[path = "module/module.rs"]
/// Kernel integration model and module metadata vocabulary.
pub mod module;

pub use blueprint::*;
pub use evidence::*;
pub use module::*;

#[cfg(test)]
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
    }

    #[test]
    fn blueprint_requires_explicit_unsafe_ledger() {
        assert_eq!(
            FUSION_KN_BLUEPRINT.unsafe_boundary_policy,
            KernelUnsafeBoundaryPolicy::ExplicitLedgerRequired
        );
        assert!(!FUSION_KN_BLUEPRINT.evidence.is_empty());
    }
}
