//! Capability vocabulary for PAL user-space context switching.

use bitflags::bitflags;

/// Shared authority bitset specialized for context support.
///
/// Context switching does not currently use the `TOPOLOGY` flag, but it reuses the
/// canonical bit layout so context evidence can be composed safely with thread and
/// hardware evidence later without manual remapping.
pub use crate::pal::caps::AuthoritySet as ContextAuthoritySet;
/// Shared guarantee ladder specialized for context support.
///
/// Context providers currently use a subset of this ladder in practice; `Controllable` is
/// reserved but not presently emitted by the context backends.
pub use crate::pal::caps::Guarantee as ContextGuarantee;
/// Shared implementation-category vocabulary specialized for context support.
pub use crate::pal::caps::ImplementationKind as ContextImplementationKind;

bitflags! {
    /// Set of raw context-switching operations a backend can honestly surface.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ContextCaps: u32 {
        /// The backend can create a fresh context on a supplied stack.
        const MAKE                   = 1 << 0;
        /// The backend can swap the current context with another saved context.
        const SWAP                   = 1 << 1;
        /// The backend can report the architectural stack growth direction.
        const STACK_DIRECTION        = 1 << 2;
        /// The backend can describe thread-local-storage sharing or isolation honestly.
        const TLS_ISOLATION          = 1 << 3;
        /// The backend can describe whether contexts may resume on a different carrier.
        const CROSS_CARRIER_RESUME   = 1 << 4;
        /// The backend can report signal-mask preservation semantics honestly.
        const SIGNAL_MASK_PRESERVED  = 1 << 5;
        /// The backend can state whether a guard page is required for safe stack setup.
        const GUARD_REQUIRED         = 1 << 6;
    }
}

/// Architectural stack growth direction relevant to raw context setup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContextStackDirection {
    /// The backend cannot characterize stack growth honestly.
    Unknown,
    /// Stacks grow toward lower addresses.
    Down,
    /// Stacks grow toward higher addresses.
    Up,
}

/// Truthful TLS relationship between the carrier thread and switched context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContextTlsIsolation {
    /// The backend cannot provide a meaningful TLS story.
    Unsupported,
    /// The switched context shares the carrier thread's TLS domain.
    SharedCarrierThread,
    /// The backend can provide a distinct fiber- or context-local TLS surface.
    SeparateFiberLocal,
}

/// Whether a context may be resumed on a different carrier thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContextMigrationSupport {
    /// Cross-carrier resume is not supported.
    Unsupported,
    /// A context may only resume on the carrier that created or last resumed it.
    SameCarrierOnly,
    /// The backend can honestly support cross-carrier migration.
    CrossCarrier,
}

/// Full truthful capability surface for user-space context switching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContextSupport {
    /// Supported raw context operations.
    pub caps: ContextCaps,
    /// Overall strength of the context-switch guarantee.
    pub guarantee: ContextGuarantee,
    /// Minimum alignment required for the top-of-stack handoff.
    pub min_stack_alignment: usize,
    /// Architectural red-zone size below the active stack pointer in bytes.
    pub red_zone_bytes: usize,
    /// Architectural stack direction used by this backend.
    pub stack_direction: ContextStackDirection,
    /// Whether the backend requires a guard page or equivalent stack limit mechanism.
    pub guard_required: bool,
    /// TLS sharing or isolation semantics.
    pub tls_isolation: ContextTlsIsolation,
    /// Whether signal masks are preserved across a context switch.
    pub signal_mask_preserved: bool,
    /// Whether unwinding across the context-switch boundary is supported.
    pub unwind_across_boundary: bool,
    /// Cross-carrier migration support.
    pub migration: ContextMigrationSupport,
    /// Authorities contributing to this support record.
    pub authorities: ContextAuthoritySet,
    /// Native, emulated, or unsupported implementation category.
    pub implementation: ContextImplementationKind,
}

impl ContextSupport {
    /// Returns a fully unsupported context-switch surface.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            caps: ContextCaps::empty(),
            guarantee: ContextGuarantee::Unsupported,
            min_stack_alignment: 1,
            red_zone_bytes: 0,
            stack_direction: ContextStackDirection::Unknown,
            guard_required: false,
            tls_isolation: ContextTlsIsolation::Unsupported,
            signal_mask_preserved: false,
            unwind_across_boundary: false,
            migration: ContextMigrationSupport::Unsupported,
            authorities: ContextAuthoritySet::empty(),
            implementation: ContextImplementationKind::Unsupported,
        }
    }
}
