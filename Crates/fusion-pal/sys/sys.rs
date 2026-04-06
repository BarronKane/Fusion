//! Selected platform facade wiring for the fusion-pal.
//!
//! The raw backend families live under [`crate::pal`]. This module consumes those PAL families and
//! exposes the canonical selected public surface through uniform paths such as
//! `fusion_pal::sys::mem`.
//!
//! Safety-critical note:
//! This module is the top of the selected public boundary for `fusion-pal`. Any dependency,
//! syscall wrapper, foreign-function interface, or operating-system import used beneath
//! [`crate::pal`] becomes part of the assurance case for the selected target.
//! That matters directly for higher-assurance environments governed by standards such as
//! DO-178C and IEC 62304, where the auditability and qualification story of every boundary
//! crossing is not optional just because the code is fashionable and written in Rust.
//!
//! In practice, different platforms offer very different levels of control:
//! - Linux exposes a stable syscall ABI, so the backend can in principle own the full path
//!   from Rust to kernel entry. The current Linux backend uses `rustix` for that boundary,
//!   but this is an implementation choice rather than a philosophical requirement, and it
//!   may be replaced with a thinner directly owned syscall layer if stricter qualification
//!   or traceability requirements demand it.
//! - Apple targets do not offer a public stable syscall ABI in the same way, so serious fusion-pal
//!   implementations are typically forced through `libSystem` or equivalent system-provided
//!   foreign interfaces.
//! - Windows similarly pushes user-mode code through system DLL boundaries rather than a
//!   public stable raw syscall contract suitable for a portable fusion-pal to own directly.
//!
//! The fusion-pal therefore treats backend truth and backend auditability as first-class concerns.
//! Some targets may be able to support a stronger critical-safety claim than others, and the
//! library should say that plainly rather than smoothing the boundary over with “portable”
//! abstractions that become fiction under real assurance scrutiny.

#[cfg(not(any(
    all(target_os = "none", feature = "sys-cortex-m"),
    target_os = "ios",
    target_os = "linux",
    target_os = "macos",
    target_os = "windows"
)))]
compile_error!(
    "fusion-pal currently supports Linux, Windows, macOS, iOS, and Cortex-M (feature `sys-cortex-m`) targets."
);

#[cfg(all(feature = "sys-cortex-m", not(target_os = "none")))]
compile_error!(
    "fusion-pal feature `sys-cortex-m` requires a bare-metal target (target_os = \"none\")."
);

#[cfg(all(feature = "soc-rp2350", not(feature = "sys-cortex-m")))]
compile_error!("fusion-pal Cortex-M SoC features require `sys-cortex-m`.");

#[cfg(all(feature = "sys-fusion-kn", not(target_os = "linux")))]
compile_error!("fusion-pal feature `sys-fusion-kn` currently supports only Linux targets.");

/// Hosted implementation-family facade.
pub mod hosted {
    pub use crate::pal::hosted::*;
}

#[path = "soc/soc.rs"]
/// Static SoC implementation-family facade.
pub mod soc;

#[cfg(target_os = "ios")]
use crate::pal::hosted::ios as platform;
#[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
use crate::pal::soc::cortex_m as platform;

#[cfg(all(target_os = "linux", not(feature = "sys-fusion-kn")))]
use crate::pal::hosted::linux as platform;

#[cfg(all(target_os = "linux", feature = "sys-fusion-kn"))]
use crate::pal::hosted::fusion_kn as platform;

#[cfg(target_os = "macos")]
use crate::pal::hosted::macos as platform;

#[cfg(target_os = "windows")]
use crate::pal::hosted::windows as platform;

#[path = "vector_runtime.rs"]
mod vector_runtime;

#[path = "runtime_progress.rs"]
pub mod runtime_progress;

/// Public execution-context module re-exported from the selected platform backend.
pub mod execution_context {
    pub use super::platform::context::{
        PlatformContext,
        PlatformSavedContext,
        system_context,
        system_context_support,
    };
    pub use crate::contract::pal::runtime::context::*;
}
/// Public atomic substrate module re-exported from the selected platform backend.
pub mod atomic {
    pub use super::platform::atomic::{
        PLATFORM_ATOMIC_WAIT_WORD32_IMPLEMENTATION,
        PLATFORM_ATOMIC_WORD32_IMPLEMENTATION,
        PlatformAtomic,
        PlatformAtomicWord32,
        system_atomic,
    };
    pub use crate::contract::pal::runtime::atomic::*;
}
/// Public claims contract surface.
pub mod claims {
    pub use crate::contract::pal::claims::*;
}
/// Public native visible-context contract surface.
pub mod context {
    pub use crate::contract::pal::domain::{
        ContextBaseContract,
        ContextCaps,
        ContextId,
        ContextImplementationKind,
        ContextKind,
        ContextProjectionKind,
        ContextSupport,
        CourierId,
        DomainError,
        DomainErrorKind,
        DomainId,
        UnsupportedContext,
    };
}
/// Public CPU- and topology-oriented hardware module re-exported from the selected backend.
pub mod cpu {
    #[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
    pub use super::platform::hal::core;
    #[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
    pub use super::platform::hal::soc;
    pub use super::platform::hal::{
        PLATFORM_CACHE_LINE_ALIGN_BYTES,
        PlatformCachePadded as CachePadded,
        PlatformHardware as PlatformCpu,
        system_hardware as system_cpu,
    };
}
/// Public DMA catalog module re-exported from the selected platform backend.
pub mod dma {
    pub use super::platform::dma::{
        PlatformDma,
        dma_controllers,
        dma_requests,
        system_dma,
    };
    pub use crate::contract::pal::dma::*;

    /// Recommended transfer shape for one DMA request consumer.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum DmaTransferShape {
        /// A memory-to-memory transfer path.
        MemoryToMemory,
        /// A memory-to-peripheral transfer path.
        MemoryToPeripheral,
        /// A peripheral-to-memory transfer path.
        PeripheralToMemory,
        /// One channel-to-channel chaining or trigger path.
        ChannelChaining,
    }

    /// Consumer-facing role inferred from one DMA request descriptor.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum DmaConsumerRole {
        /// Peripheral transmit or drain pacing.
        PeripheralTx,
        /// Peripheral receive or fill pacing.
        PeripheralRx,
        /// Peripheral-generated pacing that is not plain TX/RX FIFO traffic.
        PeripheralPacer,
        /// Timer-driven pacing.
        TimerPacer,
        /// Software-forced request with no peripheral endpoint.
        Force,
    }

    /// Consumer-side routing and pacing policy for one DMA request.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct DmaRequestPolicy {
        /// Coarse consumer role of this request.
        pub role: DmaConsumerRole,
        /// Preferred transfer shape when one is implied by the request.
        pub preferred_shape: Option<DmaTransferShape>,
    }

    /// Typed helper over one selected DMA request descriptor.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct DmaRequest {
        descriptor: &'static DmaRequestDescriptor,
    }

    impl DmaRequest {
        /// Returns all surfaced DMA request descriptors for the selected backend.
        #[must_use]
        pub fn all() -> &'static [DmaRequestDescriptor] {
            dma_requests()
        }

        /// Returns all surfaced DMA controller descriptors for the selected backend.
        #[must_use]
        pub fn controllers() -> &'static [DmaControllerDescriptor] {
            dma_controllers()
        }

        /// Looks up one DMA request by its hardware request-line selector.
        #[must_use]
        pub fn from_request_line(request_line: u16) -> Option<Self> {
            Self::all()
                .iter()
                .find(|descriptor| descriptor.request_line == request_line)
                .map(|descriptor| Self { descriptor })
        }

        /// Returns the underlying descriptor.
        #[must_use]
        pub const fn descriptor(self) -> &'static DmaRequestDescriptor {
            self.descriptor
        }

        /// Returns the hardware request-line selector.
        #[must_use]
        pub const fn request_line(self) -> u16 {
            self.descriptor.request_line
        }

        /// Returns the associated peripheral block when one exists.
        #[must_use]
        pub const fn peripheral(self) -> Option<&'static str> {
            self.descriptor.peripheral
        }

        /// Returns the peripheral-local endpoint name when one exists.
        #[must_use]
        pub const fn endpoint(self) -> Option<&'static str> {
            self.descriptor.endpoint
        }

        /// Returns the coarse request classification.
        #[must_use]
        pub const fn class(self) -> DmaRequestClass {
            self.descriptor.class
        }

        /// Returns the transfer capability envelope of this request.
        #[must_use]
        pub const fn transfer_caps(self) -> DmaTransferCaps {
            self.descriptor.transfer_caps
        }

        /// Returns the consumer-facing routing and pacing policy for this request.
        #[must_use]
        pub const fn policy(self) -> DmaRequestPolicy {
            policy_for_descriptor(self.descriptor)
        }

        /// Returns the consumer-facing role inferred for this request.
        #[must_use]
        pub const fn consumer_role(self) -> DmaConsumerRole {
            self.policy().role
        }

        /// Returns the preferred transfer shape implied by this request, when one exists.
        #[must_use]
        pub const fn preferred_shape(self) -> Option<DmaTransferShape> {
            self.policy().preferred_shape
        }

        /// Returns whether the descriptor honestly supports the requested transfer shape.
        #[must_use]
        pub const fn supports_shape(self, shape: DmaTransferShape) -> bool {
            let caps = self.transfer_caps();
            match shape {
                DmaTransferShape::MemoryToMemory => {
                    caps.contains(DmaTransferCaps::MEMORY_TO_MEMORY)
                }
                DmaTransferShape::MemoryToPeripheral => {
                    caps.contains(DmaTransferCaps::MEMORY_TO_PERIPHERAL)
                }
                DmaTransferShape::PeripheralToMemory => {
                    caps.contains(DmaTransferCaps::PERIPHERAL_TO_MEMORY)
                }
                DmaTransferShape::ChannelChaining => {
                    caps.contains(DmaTransferCaps::CHANNEL_CHAINING)
                }
            }
        }
    }

    /// Returns the consumer-facing policy for one raw DMA request descriptor.
    #[must_use]
    pub const fn policy_for_descriptor(descriptor: &DmaRequestDescriptor) -> DmaRequestPolicy {
        match descriptor.class {
            DmaRequestClass::PeripheralTx => DmaRequestPolicy {
                role: DmaConsumerRole::PeripheralTx,
                preferred_shape: Some(DmaTransferShape::MemoryToPeripheral),
            },
            DmaRequestClass::PeripheralRx => DmaRequestPolicy {
                role: DmaConsumerRole::PeripheralRx,
                preferred_shape: Some(DmaTransferShape::PeripheralToMemory),
            },
            DmaRequestClass::PeripheralPacer => DmaRequestPolicy {
                role: DmaConsumerRole::PeripheralPacer,
                preferred_shape: None,
            },
            DmaRequestClass::TimerPacer => DmaRequestPolicy {
                role: DmaConsumerRole::TimerPacer,
                preferred_shape: None,
            },
            DmaRequestClass::Force => DmaRequestPolicy {
                role: DmaConsumerRole::Force,
                preferred_shape: Some(DmaTransferShape::MemoryToMemory),
            },
        }
    }
}
/// Public courier contract surface.
pub mod courier {
    pub use crate::contract::pal::domain::{
        ContextId,
        CourierBaseContract,
        CourierCaps,
        CourierId,
        CourierImplementationKind,
        CourierSupport,
        CourierVisibility,
        CourierVisibilityControlContract,
        DomainError,
        DomainErrorKind,
        DomainId,
        UnsupportedCourier,
    };
}
/// Public channel contract surface.
pub mod channel {
    pub use crate::contract::pal::interconnect::channel::*;
}
/// Public insight/debug side-channel contract surface.
pub mod insight {
    pub use crate::contract::pal::interconnect::insight::*;
}
/// Public event module re-exported from the selected platform backend.
pub mod event {
    pub use super::platform::event::{
        PlatformEvent,
        PlatformPoller,
        system_event,
    };
    pub use crate::contract::pal::runtime::event::*;
}
/// Public hosted-fiber helper module re-exported from the selected platform backend.
pub mod fiber {
    pub use super::platform::fiber::{
        PlatformFiberHost,
        PlatformFiberSignalStack,
        PlatformFiberWakeSignal,
        system_fiber_host,
    };
    pub use crate::contract::pal::runtime::fiber::{
        FiberHostError,
        FiberHostErrorKind,
        FiberHostSupport,
        PlatformElasticFaultHandler,
        PlatformWakeToken,
    };
}
#[cfg(feature = "sys-fusion-kn")]
/// Public mediated Fusion kernel backend module.
pub mod fusion_kn {
    pub use super::hosted::fusion_kn::*;
}
/// Public protocol contract surface.
pub mod protocol {
    pub use crate::contract::pal::interconnect::protocol::*;
}
/// Public native domain contract surface.
pub mod domain {
    pub use crate::contract::pal::domain::{
        CourierId,
        DomainBaseContract,
        DomainCaps,
        DomainError,
        DomainErrorKind,
        DomainId,
        DomainImplementationKind,
        DomainKind,
        DomainSupport,
        UnsupportedDomain,
    };
}
/// Public memory module re-exported from the selected platform backend.
pub mod mem {
    pub use super::platform::mem::{
        PlatformMem,
        system_mem,
    };
    pub use crate::contract::pal::mem::*;
}
/// Public programmable-IO module re-exported from the selected platform backend.
pub mod pcu {
    pub use super::platform::pcu::{
        PlatformPcu,
        system_pcu,
    };
    pub use crate::contract::drivers::pcu::*;
}
/// Public power module re-exported from the selected platform backend.
pub mod power {
    pub use super::platform::power::{
        PlatformPower,
        system_power,
    };
    pub use crate::contract::pal::power::*;
}
/// Public synchronization module re-exported from the selected platform backend.
pub mod sync {
    pub use super::platform::sync::{
        PLATFORM_RAW_MUTEX_IMPLEMENTATION,
        PLATFORM_RAW_ONCE_IMPLEMENTATION,
        PLATFORM_RAW_RWLOCK_IMPLEMENTATION,
        PlatformRawMutex,
        PlatformRawOnce,
        PlatformRawRwLock,
        PlatformSemaphore,
        PlatformSync,
        system_sync,
    };
    pub use crate::contract::pal::runtime::sync::*;
}
/// Public thread module re-exported from the selected platform backend.
pub mod thread {
    pub use super::platform::thread::{
        PlatformThread,
        PlatformThreadHandle,
        system_thread,
    };
    pub use crate::contract::pal::runtime::thread::*;
}
/// Public transport-layer contract surface.
pub mod transport {
    pub use crate::contract::pal::interconnect::transport::*;
}
/// Public vector module re-exported from the selected platform backend.
pub mod vector {
    pub use super::platform::vector::{
        bind_reserved_event_timeout_wake,
        PlatformSealedVectorTable,
        PlatformVector,
        PlatformVectorBuilder,
        bind_reserved_pendsv_dispatch,
        system_vector,
        take_pending_active_scope,
    };
    pub use super::vector_runtime::{
        ensure_runtime_reserved_wake_vectors,
        ensure_runtime_reserved_wake_vectors_best_effort,
        with_runtime_vector_builder,
    };
    pub use crate::contract::pal::vector::*;
}
