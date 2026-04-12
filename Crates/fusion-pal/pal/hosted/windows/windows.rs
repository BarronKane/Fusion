#[path = "context/context.rs"]
/// Windows fusion-pal user-space context backend implementation.
pub mod context;
#[path = "dma/dma.rs"]
/// Windows fusion-pal DMA backend implementation.
pub mod dma;
/// Windows hosted machine identity surface.
pub mod identity {
    use std::sync::OnceLock;

    use windows::core::PWSTR;
    use windows::Win32::System::SystemInformation::{
        ComputerNamePhysicalDnsHostname,
        GetComputerNameExW,
    };

    static DOMAIN_NAME: OnceLock<String> = OnceLock::new();

    /// Returns the canonical local-domain name for the current hosted machine.
    #[must_use]
    pub fn system_domain_name() -> &'static str {
        DOMAIN_NAME.get_or_init(|| {
            const FALLBACK: &str = "windows";

            let mut stack = [0u16; 256];
            let mut len = stack.len() as u32;

            if unsafe {
                GetComputerNameExW(
                    ComputerNamePhysicalDnsHostname,
                    Some(PWSTR(stack.as_mut_ptr())),
                    &mut len,
                )
            }
            .is_ok()
            {
                let name = String::from_utf16_lossy(&stack[..len as usize])
                    .trim()
                    .to_owned();
                return if name.is_empty() {
                    FALLBACK.to_owned()
                } else {
                    name
                };
            }

            if len == 0 {
                return FALLBACK.to_owned();
            }

            let mut heap = vec![0u16; len as usize];
            if unsafe {
                GetComputerNameExW(
                    ComputerNamePhysicalDnsHostname,
                    Some(PWSTR(heap.as_mut_ptr())),
                    &mut len,
                )
            }
            .is_ok()
            {
                let name = String::from_utf16_lossy(&heap[..len as usize])
                    .trim()
                    .to_owned();
                if !name.is_empty() {
                    return name;
                }
            }

            FALLBACK.to_owned()
        })
    }
}
/// Windows hosted process entry remains ambient to the operating system.
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
#[path = "atomic/atomic.rs"]
/// Windows fusion-pal atomic backend implementation.
pub mod atomic;
#[path = "event/event.rs"]
/// Windows fusion-pal event backend implementation.
pub mod event;
#[path = "fiber/fiber.rs"]
/// Windows hosted-fiber helper backend implementation.
pub mod fiber;
#[path = "hal/hal.rs"]
/// Windows fusion-pal hardware backend implementation.
pub mod hal;
#[path = "mem/mem.rs"]
/// Windows fusion-pal memory backend implementation.
pub mod mem;
#[path = "pcu/pcu.rs"]
/// Windows fusion-pal programmable-IO backend implementation.
pub mod pcu;
#[path = "power/power.rs"]
/// Windows fusion-pal power backend implementation.
pub mod power;
#[path = "sync/sync.rs"]
/// Windows fusion-pal synchronization backend implementation.
pub mod sync;
#[path = "thread/thread.rs"]
/// Windows fusion-pal thread backend implementation.
pub mod thread;
/// Windows fusion-pal vector-ownership surface remains unsupported for now.
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
