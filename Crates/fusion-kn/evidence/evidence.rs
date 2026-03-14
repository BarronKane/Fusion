//! Evidence-planning vocabulary for future assurance work.
//!
//! The goal here is not to pretend that a Linux out-of-tree module is magically compliant.
//! The goal is to record the areas that will need explicit evidence if this crate is going
//! to participate in any serious safety or assurance story later.

/// Coarse evidence areas the kernel-facing crate must eventually address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KernelEvidenceArea {
    /// Requirements, architecture, and implementation traceability.
    RequirementsTraceability,
    /// Toolchain and build reproducibility control.
    ToolchainControl,
    /// Explicit catalog of unsafe Rust and kernel FFI boundaries.
    UnsafeBoundaryLedger,
    /// Panic, abort, and fault-containment discipline.
    PanicDiscipline,
    /// Dynamic allocation policy and justification.
    AllocationDiscipline,
    /// Initialization and shutdown sequencing discipline.
    InitializationDiscipline,
    /// Verification, review, and test evidence mapping.
    VerificationMapping,
}

/// Current planning status for a given evidence area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KernelEvidenceStatus {
    /// The evidence area is identified but not yet implemented.
    Planned,
    /// Work has started but the area is not yet satisfied.
    InProgress,
    /// The area has been satisfied to the current project standard.
    Satisfied,
    /// The area is intentionally deferred.
    Deferred,
}

/// One evidence expectation recorded for the kernel-facing crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KernelEvidenceExpectation {
    /// Evidence area being tracked.
    pub area: KernelEvidenceArea,
    /// Current planning or completion status.
    pub status: KernelEvidenceStatus,
    /// Short rationale for why the evidence item exists.
    pub rationale: &'static str,
}

/// Baseline evidence plan for the initial `fusion-kn` blueprint.
pub const DO_178C_KERNEL_BASELINE: [KernelEvidenceExpectation; 7] = [
    KernelEvidenceExpectation {
        area: KernelEvidenceArea::RequirementsTraceability,
        status: KernelEvidenceStatus::Planned,
        rationale: "Kernel-facing requirements, design, code, and verification need an auditable chain.",
    },
    KernelEvidenceExpectation {
        area: KernelEvidenceArea::ToolchainControl,
        status: KernelEvidenceStatus::Planned,
        rationale: "The Rust-for-Linux build path and kernel tree inputs must be versioned and repeatable.",
    },
    KernelEvidenceExpectation {
        area: KernelEvidenceArea::UnsafeBoundaryLedger,
        status: KernelEvidenceStatus::Planned,
        rationale: "Unsafe Rust, raw kernel APIs, and cross-language boundaries need explicit justification.",
    },
    KernelEvidenceExpectation {
        area: KernelEvidenceArea::PanicDiscipline,
        status: KernelEvidenceStatus::Planned,
        rationale: "Kernel execution cannot lean on ambient panic behavior and still claim disciplined failure handling.",
    },
    KernelEvidenceExpectation {
        area: KernelEvidenceArea::AllocationDiscipline,
        status: KernelEvidenceStatus::Planned,
        rationale: "Allocator usage and GFP-context expectations need to be explicit and reviewable.",
    },
    KernelEvidenceExpectation {
        area: KernelEvidenceArea::InitializationDiscipline,
        status: KernelEvidenceStatus::Planned,
        rationale: "Module init and teardown must define ordering, rollback, and partial-failure handling.",
    },
    KernelEvidenceExpectation {
        area: KernelEvidenceArea::VerificationMapping,
        status: KernelEvidenceStatus::Planned,
        rationale: "Static analysis, review, and test evidence need a place to attach before the crate grows real behavior.",
    },
];
