//! Cortex-M bare-metal context backend.
//!
//! The architectural stack discipline is known on Cortex-M even though the raw make/swap path
//! is not implemented yet. So this provider reports the parts it can justify and leaves the
//! actual context-switch operations unsupported until the PendSV path is wired properly.

use crate::pal::context::{
    ContextAuthoritySet, ContextBase, ContextCaps, ContextError, ContextGuarantee,
    ContextMigrationSupport, ContextStackDirection, ContextStackLayout, ContextSupport,
    ContextSwitch, ContextTlsIsolation, RawContextEntry,
};

/// Cortex-M context provider type.
#[derive(Debug, Clone, Copy, Default)]
pub struct CortexMContext;

/// Cortex-M saved-context placeholder.
#[derive(Debug, Default, PartialEq, Eq, Hash)]
pub struct CortexMSavedContext;

/// Selected Cortex-M context provider type.
pub type PlatformContext = CortexMContext;
/// Selected Cortex-M saved-context type.
pub type PlatformSavedContext = CortexMSavedContext;

const CORTEX_M_CONTEXT_SUPPORT: ContextSupport = ContextSupport {
    caps: ContextCaps::STACK_DIRECTION
        .union(ContextCaps::TLS_ISOLATION)
        .union(ContextCaps::CROSS_CARRIER_RESUME)
        .union(ContextCaps::GUARD_REQUIRED),
    guarantee: ContextGuarantee::Unsupported,
    min_stack_alignment: 8,
    red_zone_bytes: 0,
    stack_direction: ContextStackDirection::Down,
    guard_required: false,
    tls_isolation: ContextTlsIsolation::SharedCarrierThread,
    signal_mask_preserved: false,
    unwind_across_boundary: false,
    migration: ContextMigrationSupport::SameCarrierOnly,
    authorities: ContextAuthoritySet::ISA,
    implementation: crate::pal::context::ContextImplementationKind::Unsupported,
};

/// Returns the selected Cortex-M context provider.
#[must_use]
pub const fn system_context() -> PlatformContext {
    PlatformContext::new()
}

impl CortexMContext {
    /// Creates a new Cortex-M context provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl ContextBase for CortexMContext {
    type Context = CortexMSavedContext;

    fn support(&self) -> ContextSupport {
        CORTEX_M_CONTEXT_SUPPORT
    }
}

// SAFETY: this backend does not create or switch contexts until the PendSV-backed implementation
// exists, so every raw operation fails explicitly instead of pretending.
unsafe impl ContextSwitch for CortexMContext {
    unsafe fn make(
        &self,
        _stack: ContextStackLayout,
        _entry: RawContextEntry,
        _arg: *mut (),
    ) -> Result<Self::Context, ContextError> {
        Err(ContextError::unsupported())
    }

    unsafe fn swap(
        &self,
        _from: &mut Self::Context,
        _to: &Self::Context,
    ) -> Result<(), ContextError> {
        Err(ContextError::unsupported())
    }
}
