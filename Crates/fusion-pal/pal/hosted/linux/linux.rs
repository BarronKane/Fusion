#[path = "atomic/atomic.rs"]
/// Linux fusion-pal atomic backend implementation.
pub mod atomic;
#[path = "context/context.rs"]
/// Linux fusion-pal user-space context backend implementation.
pub mod context;
#[path = "dma/dma.rs"]
/// Linux fusion-pal DMA backend implementation.
pub mod dma;
/// Linux hosted machine identity surface.
pub mod identity {
    use std::sync::OnceLock;

    use rustix::system;

    static DOMAIN_NAME: OnceLock<String> = OnceLock::new();

    /// Returns the canonical local-domain name for the current hosted machine.
    #[must_use]
    pub fn system_domain_name() -> &'static str {
        DOMAIN_NAME.get_or_init(|| {
            let uname = system::uname();
            let raw = uname.nodename().to_bytes();
            let trimmed = raw
                .iter()
                .copied()
                .take_while(|byte| *byte != 0)
                .collect::<Vec<_>>();
            let name = String::from_utf8_lossy(&trimmed).trim().to_owned();
            if name.is_empty() {
                "linux".to_owned()
            } else {
                name
            }
        })
    }
}
/// Linux hosted process entry remains ambient to the operating system.
pub mod entry {
    pub use crate::contract::pal::runtime::entry::{
        EntryBaseContract,
        EntryImplementationKind,
        EntryKind,
        EntrySupport,
    };

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
            EntrySupport::ambient_hosted()
        }
    }

    #[must_use]
    pub const fn system_entry() -> PlatformEntry {
        PlatformEntry::new()
    }
}
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
        bind_reserved_event_timeout_wake,
        bind_reserved_pendsv_dispatch,
        bind_reserved_runtime_dispatch,
        request_reserved_pendsv_dispatch,
        take_pending_active_scope,
    };

    /// Returns the unsupported vector provider for the selected backend.
    #[must_use]
    pub const fn system_vector() -> PlatformVector {
        PlatformVector::new()
    }
}
