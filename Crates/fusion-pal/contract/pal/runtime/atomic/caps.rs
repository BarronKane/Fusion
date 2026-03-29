use bitflags::bitflags;

/// Shared implementation-category vocabulary specialized for atomic support.
pub use crate::contract::pal::caps::ImplementationKind as AtomicImplementationKind;

/// Atomic-specific fallback or degradation strategy reported alongside implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AtomicFallbackKind {
    /// No special fallback or degradation applies.
    None,
    /// The primitive falls back to a local interrupt-masked critical section.
    CriticalSection,
}

/// Sharing scope offered by a waitable atomic surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AtomicScopeSupport {
    /// The primitive is local to the current process or execution image.
    LocalOnly,
    /// The primitive can be shared across processes when backed by shared memory.
    ProcessShared,
}

bitflags! {
    /// Timeout semantics supported by an atomic wait surface.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct AtomicTimeoutCaps: u32 {
        /// Supports relative timeouts.
        const RELATIVE = 1 << 0;
        /// Relative timeouts are measured against a monotonic clock.
        const RELATIVE_MONOTONIC = 1 << 1;
        /// Supports absolute timeouts on a monotonic clock.
        const ABSOLUTE_MONOTONIC = 1 << 2;
        /// Supports absolute timeouts on a realtime/wall clock.
        const ABSOLUTE_REALTIME = 1 << 3;
    }
}

bitflags! {
    /// Capability flags for one 32-bit atomic word surface.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct AtomicWord32Caps: u32 {
        /// Supports acquire/release/relaxed loads.
        const LOAD = 1 << 0;
        /// Supports acquire/release/relaxed stores.
        const STORE = 1 << 1;
        /// Supports swap/exchange operations.
        const SWAP = 1 << 2;
        /// Supports compare-and-exchange operations.
        const COMPARE_EXCHANGE = 1 << 3;
        /// Supports fetch-add operations.
        const FETCH_ADD = 1 << 4;
        /// Supports fetch-sub operations.
        const FETCH_SUB = 1 << 5;
        /// Supports fetch-and operations.
        const FETCH_AND = 1 << 6;
        /// Supports fetch-or operations.
        const FETCH_OR = 1 << 7;
        /// Supports fetch-xor operations.
        const FETCH_XOR = 1 << 8;
        /// Supports static initialization without heap allocation.
        const STATIC_INIT = 1 << 9;
    }
}

bitflags! {
    /// Capability flags for wait/wake over one 32-bit atomic word.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct AtomicWaitWord32Caps: u32 {
        /// Supports waiting while a word remains equal to an expected value.
        const WAIT_WHILE_EQUAL = 1 << 0;
        /// Supports waking a single waiter.
        const WAKE_ONE = 1 << 1;
        /// Supports waking all waiters on a word.
        const WAKE_ALL = 1 << 2;
        /// Wait operations may return spuriously and require caller-side rechecking.
        const SPURIOUS_WAKE = 1 << 3;
    }
}

/// Support surface for one 32-bit atomic word implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AtomicWord32Support {
    /// Fine-grained 32-bit atomic-word capabilities.
    pub caps: AtomicWord32Caps,
    /// Whether the backend implementation is native, emulated, or unavailable.
    pub implementation: AtomicImplementationKind,
    /// Atomic-specific fallback detail, if any.
    pub fallback: AtomicFallbackKind,
}

impl AtomicWord32Support {
    /// Returns an explicitly unsupported 32-bit atomic-word surface.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            caps: AtomicWord32Caps::empty(),
            implementation: AtomicImplementationKind::Unsupported,
            fallback: AtomicFallbackKind::None,
        }
    }
}

/// Support surface for waiting and waking over one 32-bit atomic word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AtomicWaitWord32Support {
    /// Fine-grained wait/wake capabilities.
    pub caps: AtomicWaitWord32Caps,
    /// Supported timeout models for wait operations.
    pub timeout: AtomicTimeoutCaps,
    /// Sharing scope semantics, if any.
    pub scope: AtomicScopeSupport,
    /// Whether the backend implementation is native, emulated, or unavailable.
    pub implementation: AtomicImplementationKind,
    /// Atomic-specific fallback detail, if any.
    pub fallback: AtomicFallbackKind,
}

impl AtomicWaitWord32Support {
    /// Returns an explicitly unsupported 32-bit wait/wake surface.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            caps: AtomicWaitWord32Caps::empty(),
            timeout: AtomicTimeoutCaps::empty(),
            scope: AtomicScopeSupport::LocalOnly,
            implementation: AtomicImplementationKind::Unsupported,
            fallback: AtomicFallbackKind::None,
        }
    }
}

/// Aggregated atomic support surface for a backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AtomicSupport {
    /// 32-bit atomic-word support.
    pub word32: AtomicWord32Support,
    /// 32-bit wait/wake support.
    pub wait_word32: AtomicWaitWord32Support,
}

impl AtomicSupport {
    /// Returns a backend with no supported atomic surfaces.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            word32: AtomicWord32Support::unsupported(),
            wait_word32: AtomicWaitWord32Support::unsupported(),
        }
    }
}
