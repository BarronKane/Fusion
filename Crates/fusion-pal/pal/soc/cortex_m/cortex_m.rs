#[path = "atomic/atomic.rs"]
/// Cortex-M bare-metal atomic backend implementation.
pub mod atomic;
#[path = "context/context.rs"]
/// Cortex-M bare-metal context backend implementation.
pub mod context;
#[path = "event/event.rs"]
/// Cortex-M bare-metal event backend implementation.
pub mod event;
#[path = "gpio/gpio.rs"]
/// Cortex-M GPIO backend implementation backed by selected static topology.
pub mod gpio;
/// Cortex-M hosted-fiber surface (unsupported — fibers are managed directly).
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
/// Cortex-M hardware backend implementation.
pub mod hal;
#[path = "mem/mem.rs"]
/// Cortex-M bare-metal memory backend implementation.
pub mod mem;
#[path = "pcu/pcu.rs"]
/// Cortex-M bare-metal coprocessor backend implementation, including PIO-specific lanes.
pub mod pcu;
#[path = "power/power.rs"]
/// Cortex-M bare-metal power backend implementation.
pub mod power;
#[cfg(feature = "soc-rp2350")]
/// RP2350 SoC surface under the Cortex-M lane.
pub mod rp2350;
#[path = "sync/sync.rs"]
/// Cortex-M bare-metal synchronization backend implementation.
pub mod sync;
#[path = "thread/thread.rs"]
/// Cortex-M bare-metal thread backend implementation.
pub mod thread;
#[path = "vector/vector.rs"]
/// Cortex-M bare-metal vector-ownership backend implementation.
pub mod vector;
