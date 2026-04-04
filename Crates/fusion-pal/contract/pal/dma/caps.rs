//! Capability vocabulary for fusion-pal DMA catalog support.

use bitflags::bitflags;

/// Shared implementation-category vocabulary specialized for DMA support.
pub use crate::contract::pal::caps::ImplementationKind as DmaImplementationKind;

bitflags! {
    /// DMA catalog features the backend can honestly surface.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct DmaCaps: u32 {
        /// The backend can enumerate DMA controllers.
        const ENUMERATE_CONTROLLERS = 1 << 0;
        /// The backend can enumerate DMA request lines.
        const ENUMERATE_REQUESTS    = 1 << 1;
    }
}

/// Full capability surface for one DMA-capable backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DmaSupport {
    /// Backend-supported DMA catalog features.
    pub caps: DmaCaps,
    /// Native, emulated, or unsupported implementation category.
    pub implementation: DmaImplementationKind,
}

impl DmaSupport {
    /// Returns a fully unsupported DMA surface.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            caps: DmaCaps::empty(),
            implementation: DmaImplementationKind::Unsupported,
        }
    }
}

bitflags! {
    /// Supported DMA transfer shapes surfaced by a backend.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct DmaTransferCaps: u32 {
        /// DMA can copy between ordinary memory endpoints.
        const MEMORY_TO_MEMORY     = 1 << 0;
        /// DMA can copy from memory to one peripheral endpoint.
        const MEMORY_TO_PERIPHERAL = 1 << 1;
        /// DMA can copy from one peripheral endpoint to memory.
        const PERIPHERAL_TO_MEMORY = 1 << 2;
        /// DMA can chain or trigger one channel from another.
        const CHANNEL_CHAINING     = 1 << 3;
    }
}

/// Static DMA controller descriptor surfaced by a backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DmaControllerDescriptor {
    /// Human-readable DMA controller name.
    pub name: &'static str,
    /// Base address of the controller register block.
    pub base: usize,
    /// Number of hardware channels exposed by the controller.
    pub channel_count: u8,
    /// Coarse transfer capabilities supported by the controller.
    pub transfer_caps: DmaTransferCaps,
}

/// Coarse class of one DMA request line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DmaRequestClass {
    /// Peripheral transmit-side pacing or drain request.
    PeripheralTx,
    /// Peripheral receive-side pacing or fill request.
    PeripheralRx,
    /// Peripheral-generated pacing request that is not a plain TX/RX FIFO endpoint.
    PeripheralPacer,
    /// DMA timer pacing source.
    TimerPacer,
    /// Unconditional software-force request.
    Force,
}

/// Static DMA request descriptor surfaced by a backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DmaRequestDescriptor {
    /// Human-readable request-line name.
    pub name: &'static str,
    /// Backend-defined request-line selector.
    pub request_line: u16,
    /// Peripheral associated with the request line when one exists.
    pub peripheral: Option<&'static str>,
    /// Coarse request classification for routing and pacing semantics.
    pub class: DmaRequestClass,
    /// Peripheral-local endpoint selector when one exists.
    pub endpoint: Option<&'static str>,
    /// Coarse transfer capabilities supported by this request line.
    pub transfer_caps: DmaTransferCaps,
}
