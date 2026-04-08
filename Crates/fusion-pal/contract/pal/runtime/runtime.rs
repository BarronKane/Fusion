//! PAL runtime-substrate contracts.
//!
//! Bare-metal runtime doctrine:
//! - the runtime entry boundary on bare metal belongs to Fusion, not to each example or
//!   application crate
//! - the selected PAL/firmware lane owns reset-to-runtime handoff and must establish the initial
//!   Fusion execution context before ordinary user code runs
//! - the boot/current hardware execution lane should therefore be adopted as the first carrier,
//!   with the root courier bound there immediately
//! - the selected target entry surface should therefore live behind one canonical PAL path:
//!   `fusion_pal::sys::entry`
//! - any raw `#[entry]` or board-specific startup shim living in user examples is transitional
//!   bring-up residue and should be retired once the platform-owned entry path exists

#[path = "atomic/atomic.rs"]
pub mod atomic;
#[path = "context/context.rs"]
pub mod context;
#[path = "entry/entry.rs"]
pub mod entry;
#[path = "event/event.rs"]
pub mod event;
#[path = "fiber/fiber.rs"]
pub mod fiber;
#[path = "sync/sync.rs"]
pub mod sync;
#[path = "thread/thread.rs"]
pub mod thread;

pub use thread::ThreadConfig;
