//! fusion-sys generic coprocessor wrappers, semantic IR, and dispatch vocabulary.
//!
//! `fusion-sys::pcu` sits one layer above the raw fusion-pal PCU contract. The generic surface is
//! intentionally thin but finally honest: device enumeration/claiming, semantic kernel IR,
//! planning/preparation/dispatch handles, and backend-specific lanes such as Cortex-M PIO that do
//! not pretend every coprocessor is one ISA with different lighting.

mod dispatch;
mod ir;
mod system;

#[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
pub mod cortex_m;

pub use dispatch::*;
pub use ir::*;
pub use system::*;

pub use fusion_pal::sys::pcu::{
    PcuBase,
    PcuByteStreamBindings,
    PcuCaps,
    PcuControl,
    PcuDeviceClaim,
    PcuDeviceClass,
    PcuDeviceDescriptor,
    PcuDeviceId,
    PcuError,
    PcuErrorKind,
    PcuHalfWordStreamBindings,
    PcuImplementationKind,
    PcuInvocation,
    PcuInvocationBindings,
    PcuInvocationShape,
    PcuSupport,
    PcuWordStreamBindings,
};
