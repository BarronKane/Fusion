//! Kernel integration model and metadata vocabulary.

use bitflags::bitflags;

/// Integration model planned for the kernel-facing crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KernelIntegrationModel {
    /// Linux Rust-for-Linux out-of-tree module flow using `Kbuild`/`Makefile`.
    LinuxOutOfTreeModule,
}

/// Metadata expected of a kernel-facing module surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KernelModuleMetadata<'a> {
    /// Module name used in Cargo, build glue, and eventual kernel registration.
    pub name: &'a str,
    /// Module authors surfaced in metadata or documentation.
    pub authors: &'a [&'a str],
    /// Short human-facing module description.
    pub description: &'a str,
    /// License string exposed at the kernel boundary.
    pub license: &'a str,
}

/// Build and integration requirements inherited from the Rust-for-Linux out-of-tree model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KernelBuildRequirements {
    /// Fine-grained build and integration requirements.
    pub caps: KernelBuildRequirementCaps,
}

bitflags! {
    /// Build and integration requirements inherited from the selected kernel model.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct KernelBuildRequirementCaps: u32 {
        /// The target kernel must be built with Rust support enabled.
        const RUST_ENABLED_KERNEL = 1 << 0;
        /// The target kernel tree must expose Rust metadata needed by Kbuild.
        const RUST_METADATA       = 1 << 1;
        /// The integration path depends on Linux Kbuild infrastructure.
        const KBUILD              = 1 << 2;
        /// Rust symbols crossing the kernel boundary are constrained by GPL export rules.
        const GPL_EXPORTS         = 1 << 3;
    }
}

/// Returns the baseline build requirements for the Rust-for-Linux out-of-tree flow.
#[must_use]
pub const fn rust_for_linux_out_of_tree_requirements() -> KernelBuildRequirements {
    KernelBuildRequirements {
        caps: KernelBuildRequirementCaps::RUST_ENABLED_KERNEL
            .union(KernelBuildRequirementCaps::RUST_METADATA)
            .union(KernelBuildRequirementCaps::KBUILD)
            .union(KernelBuildRequirementCaps::GPL_EXPORTS),
    }
}
