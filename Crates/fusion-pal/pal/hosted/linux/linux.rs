#[path = "atomic/atomic.rs"]
/// Linux fusion-pal atomic backend implementation.
pub mod atomic;
#[path = "context/context.rs"]
/// Linux fusion-pal user-space context backend implementation.
pub mod context;
#[path = "dma/dma.rs"]
/// Linux fusion-pal DMA backend implementation.
pub mod dma;
#[path = "event/event.rs"]
/// Linux fusion-pal event backend implementation.
pub mod event;
#[path = "fiber/fiber.rs"]
/// Linux fusion-pal hosted-fiber helper implementation.
pub mod fiber;
#[path = "hal/hal.rs"]
/// Linux fusion-pal hardware backend implementation.
pub mod hal;
#[path = "mem/mem.rs"]
/// Linux fusion-pal memory backend implementation.
pub mod mem;
#[path = "pcu/pcu.rs"]
/// Linux fusion-pal programmable-IO backend implementation.
pub mod pcu;
#[path = "power/power.rs"]
/// Linux fusion-pal power backend implementation.
pub mod power;
#[path = "sync/sync.rs"]
/// Linux fusion-pal synchronization backend implementation.
pub mod sync;
#[path = "thread/thread.rs"]
/// Linux fusion-pal thread backend implementation.
pub mod thread;
/// Linux fusion-pal vector-ownership surface remains unsupported for now.
pub mod vector {
    pub use crate::contract::pal::vector::{
        UnsupportedSealedVectorTable as PlatformSealedVectorTable,
        UnsupportedVector as PlatformVector,
        UnsupportedVectorBuilder as PlatformVectorBuilder,
        bind_reserved_pendsv_dispatch,
        take_pending_active_scope,
    };

    /// Returns the unsupported vector provider for the selected backend.
    #[must_use]
    pub const fn system_vector() -> PlatformVector {
        PlatformVector::new()
    }
}
