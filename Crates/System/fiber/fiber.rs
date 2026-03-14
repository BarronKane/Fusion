//! System-level stackful execution surfaces built on top of PAL context switching.
//!
//! The actual fiber primitive is intentionally only partially mapped here today. The PAL
//! context contract exists, but concrete backend switching is still reported as
//! unsupported. This module therefore exposes the final vocabulary now without inventing a
//! fake stackful runtime in the meantime.

use core::fmt;
use core::num::NonZeroUsize;
use core::ptr::NonNull;

pub use fusion_pal::sys::context::{
    ContextAuthoritySet, ContextBase, ContextCaps, ContextError, ContextErrorKind,
    ContextGuarantee, ContextImplementationKind, ContextMigrationSupport, ContextStackDirection,
    ContextStackLayout, ContextSupport, ContextSwitch, ContextTlsIsolation, PlatformContext,
    PlatformSavedContext, RawContextEntry, system_context,
};

/// Low-level fiber support derived from the selected PAL context backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FiberSupport {
    /// Raw context-switching support surfaced by the PAL backend.
    pub context: ContextSupport,
}

/// Kind of fiber failure surfaced by `fusion-sys`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FiberErrorKind {
    /// Fiber switching is unsupported on the selected backend.
    Unsupported,
    /// The supplied stack or entry configuration was invalid.
    Invalid,
    /// Resources such as stack backing were exhausted.
    ResourceExhausted,
    /// The requested operation conflicted with fiber state.
    StateConflict,
    /// The PAL context backend reported a lower-level context failure.
    Context(ContextErrorKind),
}

/// Error surfaced by the low-level fiber layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FiberError {
    kind: FiberErrorKind,
}

impl FiberError {
    /// Creates an unsupported-operation error.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            kind: FiberErrorKind::Unsupported,
        }
    }

    /// Creates an invalid-configuration error.
    #[must_use]
    pub const fn invalid() -> Self {
        Self {
            kind: FiberErrorKind::Invalid,
        }
    }

    /// Creates a resource-exhaustion error.
    #[must_use]
    pub const fn resource_exhausted() -> Self {
        Self {
            kind: FiberErrorKind::ResourceExhausted,
        }
    }

    /// Creates a state-conflict error.
    #[must_use]
    pub const fn state_conflict() -> Self {
        Self {
            kind: FiberErrorKind::StateConflict,
        }
    }

    /// Returns the concrete fiber error kind.
    #[must_use]
    pub const fn kind(self) -> FiberErrorKind {
        self.kind
    }
}

impl From<ContextError> for FiberError {
    fn from(value: ContextError) -> Self {
        Self {
            kind: FiberErrorKind::Context(value.kind()),
        }
    }
}

impl fmt::Display for FiberErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Unsupported => f.write_str("fiber switching unsupported"),
            Self::Invalid => f.write_str("invalid fiber request"),
            Self::ResourceExhausted => f.write_str("fiber resources exhausted"),
            Self::StateConflict => f.write_str("fiber state conflict"),
            Self::Context(kind) => write!(f, "context backend error: {kind}"),
        }
    }
}

impl fmt::Display for FiberError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

/// Observable lifecycle state of a stackful fiber.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FiberState {
    /// The fiber has been created but never resumed.
    Created,
    /// The fiber is currently executing on a carrier.
    Running,
    /// The fiber yielded cooperatively and may resume later.
    Suspended,
    /// The fiber completed and will not resume again.
    Completed,
}

/// Logical return value produced by a fiber entry function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FiberReturn {
    /// Opaque completion code returned by the fiber.
    pub code: usize,
}

impl FiberReturn {
    /// Creates a new opaque fiber return record.
    #[must_use]
    pub const fn new(code: usize) -> Self {
        Self { code }
    }
}

/// Fiber entry signature used by the low-level stackful runtime.
pub type FiberEntry = unsafe fn(*mut ()) -> FiberReturn;

/// Yield outcome observed when resuming a fiber.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FiberYield {
    /// The fiber yielded cooperatively and may resume later.
    Yielded,
    /// The fiber completed and returned a terminal value.
    Completed(FiberReturn),
}

/// Concrete stack reservation supplied to a fiber.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FiberStack {
    /// Base of the stack reservation.
    pub base: NonNull<u8>,
    /// Total bytes in the reservation.
    pub len: NonZeroUsize,
}

/// Stack request used when carving fiber stacks from a backing memory pool later on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FiberStackSpec {
    /// Requested stack size in bytes.
    pub size_bytes: NonZeroUsize,
    /// Requested guard size in bytes.
    pub guard_bytes: usize,
}

/// System context provider wrapper used by higher fiber layers.
#[derive(Debug, Clone, Copy)]
pub struct FiberSystem {
    inner: PlatformContext,
}

/// Planned low-level fiber primitive.
#[derive(Debug)]
pub struct Fiber {
    state: FiberState,
}

impl FiberSystem {
    /// Creates a wrapper for the selected platform context backend.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: system_context(),
        }
    }

    /// Reports the truthful context-switch surface available to fibers.
    #[must_use]
    pub fn support(&self) -> FiberSupport {
        FiberSupport {
            context: ContextBase::support(&self.inner),
        }
    }
}

impl Default for FiberSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl Fiber {
    /// Creates a low-level fiber on the supplied stack.
    ///
    /// # Errors
    ///
    /// Returns an error when the selected backend cannot honestly construct a stackful
    /// execution context yet.
    pub fn new(_stack: FiberStack, _entry: FiberEntry, _arg: *mut ()) -> Result<Self, FiberError> {
        Err(FiberError::unsupported())
    }

    /// Returns the current lifecycle state of the fiber.
    #[must_use]
    pub const fn state(&self) -> FiberState {
        self.state
    }
}
