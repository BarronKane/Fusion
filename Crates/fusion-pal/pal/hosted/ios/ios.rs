#[path = "context/context.rs"]
/// iOS fusion-pal user-space context backend implementation.
pub mod context;
#[path = "event/event.rs"]
/// iOS fusion-pal event backend implementation.
pub mod event;
/// iOS GPIO surface remains unsupported for now.
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
/// iOS hosted-fiber helper surface remains unsupported for now.
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
/// iOS fusion-pal hardware backend implementation.
pub mod hal;
#[path = "mem/mem.rs"]
/// iOS fusion-pal memory backend implementation.
pub mod mem;
#[path = "pcu/pcu.rs"]
/// iOS fusion-pal programmable-IO backend implementation.
pub mod pcu;
#[path = "power/power.rs"]
/// iOS fusion-pal power backend implementation.
pub mod power;
#[path = "sync/sync.rs"]
/// iOS fusion-pal synchronization backend implementation.
pub mod sync;
#[path = "thread/thread.rs"]
/// iOS fusion-pal thread backend implementation.
pub mod thread;
/// iOS fusion-pal vector-ownership surface remains unsupported for now.
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
