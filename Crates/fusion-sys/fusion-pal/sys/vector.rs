//! Public vector-ownership export for the selected platform backend.

/// Concrete vector provider, builder, and sealed-table types for the selected platform.
pub use super::platform::vector::{
    PlatformSealedVectorTable,
    PlatformVector,
    PlatformVectorBuilder,
    bind_reserved_pendsv_dispatch,
    system_vector,
    take_pending_active_scope,
};
/// Backend-neutral fusion-pal vector vocabulary and traits.
pub use crate::vector::*;
