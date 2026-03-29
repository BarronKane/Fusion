#[path = "context/context.rs"]
/// macOS fusion-pal user-space context backend implementation.
pub mod context;
#[path = "event/event.rs"]
/// macOS fusion-pal event backend implementation.
pub mod event;
/// macOS GPIO surface remains unsupported for now.
pub mod gpio {
    pub use crate::contract::drivers::gpio::{
        UnsupportedGpio as PlatformGpio,
        UnsupportedGpioPin as PlatformGpioPin,
    };

    /// Returns the unsupported GPIO provider for the selected backend.
    #[must_use]
    pub const fn system_gpio() -> PlatformGpio {
        PlatformGpio::new()
    }
}
/// macOS hosted-fiber helper surface remains unsupported for now.
pub mod fiber {
    pub use crate::contract::pal::runtime::fiber::{
        UnsupportedFiberHost as PlatformFiberHost,
        UnsupportedFiberSignalStack as PlatformFiberSignalStack,
        UnsupportedFiberWakeSignal as PlatformFiberWakeSignal,
    };

    /// Returns the unsupported hosted-fiber helper provider for the selected backend.
    #[must_use]
    pub const fn system_fiber_host() -> PlatformFiberHost {
        PlatformFiberHost::new()
    }
}
#[path = "hal/hal.rs"]
/// macOS fusion-pal hardware backend implementation.
pub mod hal;
#[path = "mem/mem.rs"]
/// macOS fusion-pal memory backend implementation.
pub mod mem;
#[path = "pcu/pcu.rs"]
/// macOS fusion-pal programmable-IO backend implementation.
pub mod pcu;
#[path = "power/power.rs"]
/// macOS fusion-pal power backend implementation.
pub mod power;
#[path = "sync/sync.rs"]
/// macOS fusion-pal synchronization backend implementation.
pub mod sync;
#[path = "thread/thread.rs"]
/// macOS fusion-pal thread backend implementation.
pub mod thread;
/// macOS fusion-pal vector-ownership surface remains unsupported for now.
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
