#[path = "atomic/atomic.rs"]
/// Cortex-M bare-metal atomic backend implementation.
pub mod atomic;
#[path = "context/context.rs"]
/// Cortex-M bare-metal context backend implementation.
pub mod context;
#[path = "dma/dma.rs"]
/// Cortex-M bare-metal DMA backend implementation.
pub mod dma;
/// Cortex-M bare-metal identity surface.
pub mod identity {
    /// Returns the canonical local-domain name for the selected bare-metal target.
    ///
    /// Today this defaults to the selected SoC family name until a board- or user-supplied domain
    /// name exists above it.
    #[must_use]
    pub fn system_domain_name() -> &'static str {
        match super::hal::selected_soc_name() {
            "rp2350" => "RP2350",
            other => other,
        }
    }
}
/// Cortex-M bare-metal entry boundary owned by Fusion.
pub mod entry {
    pub use crate::contract::pal::runtime::entry::{
        EntryBaseContract,
        EntryImplementationKind,
        EntryKind,
        EntrySupport,
    };

    #[doc(hidden)]
    pub use cortex_m_rt as __rt;

    #[derive(Debug, Clone, Copy, Default)]
    pub struct PlatformEntry;

    impl PlatformEntry {
        #[must_use]
        pub const fn new() -> Self {
            Self
        }
    }

    impl EntryBaseContract for PlatformEntry {
        fn support(&self) -> EntrySupport {
            EntrySupport::fusion_owned_bare_metal()
        }
    }

    #[must_use]
    pub const fn system_entry() -> PlatformEntry {
        PlatformEntry::new()
    }
}
#[path = "event/event.rs"]
/// Cortex-M bare-metal event backend implementation.
pub mod event;
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
#[path = "runtime_irq/runtime_irq.rs"]
/// Cortex-M backend-owned reserved runtime IRQ dispatch.
pub mod runtime_irq;
#[path = "sync/sync.rs"]
/// Cortex-M bare-metal synchronization backend implementation.
pub mod sync;
#[path = "thread/thread.rs"]
/// Cortex-M bare-metal thread backend implementation.
pub mod thread;
#[path = "vector/vector.rs"]
/// Cortex-M bare-metal vector-ownership backend implementation.
pub mod vector;
