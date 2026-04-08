#[path = "capability/capability.rs"]
/// macOS runtime capability probing and cache surface.
pub mod capability;
#[path = "context/context.rs"]
/// macOS fusion-pal user-space context backend implementation.
pub mod context;
#[path = "dma/dma.rs"]
/// macOS fusion-pal DMA backend implementation.
pub mod dma;
/// macOS hosted machine identity surface.
pub mod identity {
    use std::ffi::CStr;
    use std::sync::OnceLock;

    static DOMAIN_NAME: OnceLock<String> = OnceLock::new();

    /// Returns the canonical local-domain name for the current hosted machine.
    #[must_use]
    pub fn system_domain_name() -> &'static str {
        DOMAIN_NAME.get_or_init(|| {
            let mut buffer = [0_i8; 256];
            let rc = unsafe { libc::gethostname(buffer.as_mut_ptr(), buffer.len()) };
            if rc != 0 {
                return "macos".to_owned();
            }
            let name = unsafe { CStr::from_ptr(buffer.as_ptr()) }
                .to_string_lossy()
                .trim()
                .to_owned();
            if name.is_empty() {
                "macos".to_owned()
            } else {
                name
            }
        })
    }
}
/// macOS hosted process entry remains ambient to the operating system.
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
/// macOS atomic surface remains unsupported for now.
pub mod atomic {
    pub use crate::contract::pal::runtime::atomic::{
        AtomicImplementationKind,
        UnsupportedAtomic as PlatformAtomic,
        UnsupportedAtomicWord32 as PlatformAtomicWord32,
    };

    /// Backend truth for the selected 32-bit atomic-word implementation on macOS.
    pub const PLATFORM_ATOMIC_WORD32_IMPLEMENTATION: AtomicImplementationKind =
        AtomicImplementationKind::Unsupported;

    /// Backend truth for the selected 32-bit atomic wait/wake implementation on macOS.
    pub const PLATFORM_ATOMIC_WAIT_WORD32_IMPLEMENTATION: AtomicImplementationKind =
        AtomicImplementationKind::Unsupported;

    /// Returns the unsupported atomic provider for the selected backend.
    #[must_use]
    pub const fn system_atomic() -> PlatformAtomic {
        PlatformAtomic::new()
    }
}
#[path = "event/event.rs"]
/// macOS fusion-pal event backend implementation.
pub mod event;
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
