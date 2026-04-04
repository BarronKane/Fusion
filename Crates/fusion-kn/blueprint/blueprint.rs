//! High-level kernel blueprint records composed from metadata and evidence plans.

use crate::contract::KernelBoundaryContract;
use crate::evidence::{
    DO_178C_KERNEL_BASELINE,
    KernelEvidenceExpectation,
};
use crate::module::{
    KernelBuildRequirements,
    KernelIntegrationModel,
    KernelModuleMetadata,
    rust_for_linux_out_of_tree_requirements,
};

/// Current maturity phase of the kernel-facing crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KernelBlueprintPhase {
    /// Structural blueprint only; not yet a committed functional module.
    Blueprint,
    /// Early prototype integrating with the target environment.
    Prototype,
    /// Functional integration phase.
    Integration,
    /// Qualification or evidence-hardening phase.
    Qualification,
}

/// Initial blueprint record for the kernel-facing crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KernelModuleBlueprint<'a> {
    /// Module metadata and identity.
    pub metadata: KernelModuleMetadata<'a>,
    /// Kernel integration model selected for the crate.
    pub integration: KernelIntegrationModel,
    /// Build requirements inherited from the selected integration model.
    pub requirements: KernelBuildRequirements,
    /// Current maturity phase.
    pub phase: KernelBlueprintPhase,
    /// Strict boundary contract expected of the crate.
    pub boundary: &'a KernelBoundaryContract<'a>,
    /// Evidence expectations attached to the blueprint.
    pub evidence: &'a [KernelEvidenceExpectation],
}

impl<'a> KernelModuleBlueprint<'a> {
    /// Creates a new kernel module blueprint rooted in the Rust-for-Linux out-of-tree model.
    #[must_use]
    pub const fn new(
        metadata: KernelModuleMetadata<'a>,
        boundary: &'a KernelBoundaryContract<'a>,
        evidence: &'a [KernelEvidenceExpectation],
    ) -> Self {
        Self {
            metadata,
            integration: KernelIntegrationModel::LinuxOutOfTreeModule,
            requirements: rust_for_linux_out_of_tree_requirements(),
            phase: KernelBlueprintPhase::Blueprint,
            boundary,
            evidence,
        }
    }
}

/// Metadata for the initial `fusion-kn` blueprint.
pub const FUSION_KN_METADATA: KernelModuleMetadata<'static> = KernelModuleMetadata {
    name: "fusion_kn",
    authors: &["Lance Wallis"],
    description: "Fusion kernel-facing module blueprint",
    license: "GPL-2.0",
};

/// Initial kernel blueprint constant for the crate.
pub const FUSION_KN_BLUEPRINT: KernelModuleBlueprint<'static> = KernelModuleBlueprint::new(
    FUSION_KN_METADATA,
    &crate::contract::FUSION_KN_BOUNDARY_CONTRACT,
    &DO_178C_KERNEL_BASELINE,
);
