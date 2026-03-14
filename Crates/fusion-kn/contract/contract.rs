//! Kernel-boundary contract vocabulary.
//!
//! The point of this module is not to claim that a Linux module is already safety-qualified.
//! The point is to make the cross-boundary rules explicit before real functionality starts
//! leaking across them.

use bitflags::bitflags;

#[path = "wire.rs"]
/// Fixed-layout bitflat wire protocol for mediated kernel exchanges.
pub mod wire;

/// High-level role of the kernel-facing crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KernelServiceClass {
    /// Foundation or integration substrate with no public operational service yet.
    Foundation,
    /// Control-plane service coordinating state or policy.
    ControlPlane,
    /// Data-plane service on an active operational path.
    DataPlane,
    /// Diagnostic or telemetry service.
    Telemetry,
}

/// Panic policy expected at the kernel boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KernelPanicPolicy {
    /// Panic paths are to be treated as forbidden design outcomes.
    Forbidden,
    /// Panic handling is delegated to a kernel-defined abort/fault policy.
    KernelDefinedAbort,
}

/// Allocation policy expected at the kernel boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KernelAllocationPolicy {
    /// Dynamic allocation is forbidden except where explicitly proven otherwise.
    Forbidden,
    /// Allocation is allowed only through explicit kernel-facing policy and review.
    ExplicitKernelAllocator,
}

/// Required discipline for unsafe or unsafe-adjacent kernel boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KernelUnsafeBoundaryPolicy {
    /// Boundary crossings require explicit ledger entries and justification.
    ExplicitLedgerRequired,
}

/// Blocking discipline expected of the kernel-facing crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KernelBlockingPolicy {
    /// Blocking or sleeping is forbidden in the declared execution contexts.
    Forbidden,
    /// Blocking is allowed only in explicitly sleepable process contexts.
    SleepableOnly,
    /// Blocking is allowed wherever the declared contexts and subsystem permit it.
    Allowed,
}

/// User-visible kernel surfaces that may be exposed by the module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KernelUserSurface {
    /// Character or misc device node.
    CharacterDevice,
    /// Sysfs attribute or directory surface.
    Sysfs,
    /// Procfs surface.
    Procfs,
    /// Netlink-facing user boundary.
    Netlink,
    /// Debugfs surface. Useful, but safety stories rarely enjoy it.
    Debugfs,
}

bitflags! {
    /// Execution contexts in which the module expects to run.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct KernelExecutionContexts: u32 {
        /// Regular process context.
        const PROCESS  = 1 << 0;
        /// Dedicated kernel thread context.
        const KTHREAD  = 1 << 1;
        /// Workqueue callback context.
        const WORKQUEUE = 1 << 2;
        /// Softirq context.
        const SOFTIRQ  = 1 << 3;
        /// Hard IRQ context.
        const HARDIRQ  = 1 << 4;
        /// NMI context.
        const NMI      = 1 << 5;
    }
}

/// Coarse classification of a kernel boundary entry that requires review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KernelBoundaryKind {
    /// Module load/unload, registration, and teardown paths.
    ModuleLifecycle,
    /// Kernel logging and diagnostic observation paths.
    KernelLogging,
    /// Dynamic allocation or lifetime management at the kernel edge.
    Allocation,
    /// User-facing entry or control surface.
    UserSurface,
    /// Cross-language or raw foreign interface boundary.
    ForeignInterface,
}

/// One explicit boundary ledger entry recorded for the crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KernelBoundaryLedgerEntry<'a> {
    /// Stable identifier suitable for review records and traceability tables.
    pub id: &'a str,
    /// Boundary classification.
    pub kind: KernelBoundaryKind,
    /// Short safety rationale for the entry.
    pub rationale: &'a str,
    /// Whether explicit review is required before the boundary may evolve.
    pub review_required: bool,
}

/// A strict contract for what the kernel-facing crate is allowed to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KernelBoundaryContract<'a> {
    /// High-level service role for the kernel-facing crate.
    pub service_class: KernelServiceClass,
    /// Panic handling policy.
    pub panic_policy: KernelPanicPolicy,
    /// Allocation policy.
    pub allocation_policy: KernelAllocationPolicy,
    /// Boundary-ledger policy.
    pub unsafe_boundary_policy: KernelUnsafeBoundaryPolicy,
    /// Blocking policy for the declared contexts.
    pub blocking_policy: KernelBlockingPolicy,
    /// Execution contexts this crate is allowed to run within.
    pub allowed_contexts: KernelExecutionContexts,
    /// User-visible surfaces the crate is allowed to expose.
    pub user_surfaces: &'a [KernelUserSurface],
    /// Boundary ledger entries recorded for review and traceability.
    pub boundary_ledger: &'a [KernelBoundaryLedgerEntry<'a>],
}

/// Contract consistency failures that should be caught before real kernel behavior lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KernelBoundaryContractError {
    /// The policy requires a ledger, but no entries were declared.
    MissingBoundaryLedger,
    /// IRQ-like contexts were declared while blocking was still allowed.
    BlockingAllowedInNonSleepableContext,
}

impl KernelBoundaryContract<'_> {
    /// Validates that the contract is internally coherent.
    ///
    /// # Errors
    ///
    /// Returns an error when the declared contract violates one of the crate's
    /// current cross-boundary consistency rules.
    pub fn validate(&self) -> Result<(), KernelBoundaryContractError> {
        if matches!(
            self.unsafe_boundary_policy,
            KernelUnsafeBoundaryPolicy::ExplicitLedgerRequired
        ) && self.boundary_ledger.is_empty()
        {
            return Err(KernelBoundaryContractError::MissingBoundaryLedger);
        }

        let irq_like = KernelExecutionContexts::SOFTIRQ
            | KernelExecutionContexts::HARDIRQ
            | KernelExecutionContexts::NMI;
        if self.allowed_contexts.intersects(irq_like)
            && !matches!(self.blocking_policy, KernelBlockingPolicy::Forbidden)
        {
            return Err(KernelBoundaryContractError::BlockingAllowedInNonSleepableContext);
        }

        Ok(())
    }
}

/// Initial boundary ledger for `fusion-kn`.
pub const FUSION_KN_BOUNDARY_LEDGER: [KernelBoundaryLedgerEntry<'static>; 2] = [
    KernelBoundaryLedgerEntry {
        id: "module.lifecycle",
        kind: KernelBoundaryKind::ModuleLifecycle,
        rationale: "Module load and unload remain explicit kernel-boundary transitions requiring rollback discipline.",
        review_required: true,
    },
    KernelBoundaryLedgerEntry {
        id: "kernel.logging",
        kind: KernelBoundaryKind::KernelLogging,
        rationale: "Kernel logging is retained for controlled observability during early integration and must remain reviewable.",
        review_required: true,
    },
];

/// Initial strict boundary contract for `fusion-kn`.
pub const FUSION_KN_BOUNDARY_CONTRACT: KernelBoundaryContract<'static> = KernelBoundaryContract {
    service_class: KernelServiceClass::Foundation,
    panic_policy: KernelPanicPolicy::Forbidden,
    allocation_policy: KernelAllocationPolicy::ExplicitKernelAllocator,
    unsafe_boundary_policy: KernelUnsafeBoundaryPolicy::ExplicitLedgerRequired,
    blocking_policy: KernelBlockingPolicy::SleepableOnly,
    allowed_contexts: KernelExecutionContexts::PROCESS,
    user_surfaces: &[],
    boundary_ledger: &FUSION_KN_BOUNDARY_LEDGER,
};
