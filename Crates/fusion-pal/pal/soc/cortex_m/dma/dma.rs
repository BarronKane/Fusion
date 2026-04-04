//! Cortex-M bare-metal DMA catalog backend.

use crate::contract::pal::dma::{
    DmaBase,
    DmaCaps,
    DmaCatalog,
    DmaControllerDescriptor,
    DmaImplementationKind,
    DmaRequestDescriptor,
    DmaSupport,
};

/// Cortex-M DMA provider type.
#[derive(Debug, Clone, Copy, Default)]
pub struct Dma;

/// Selected Cortex-M DMA provider type.
pub type PlatformDma = Dma;

/// Returns the selected Cortex-M DMA provider.
#[must_use]
pub const fn system_dma() -> PlatformDma {
    PlatformDma::new()
}

/// Returns the surfaced DMA controllers for the selected Cortex-M board.
#[must_use]
pub fn dma_controllers() -> &'static [DmaControllerDescriptor] {
    crate::pal::soc::cortex_m::hal::soc::board::dma_controllers()
}

/// Returns the surfaced DMA request lines for the selected Cortex-M board.
#[must_use]
pub fn dma_requests() -> &'static [DmaRequestDescriptor] {
    crate::pal::soc::cortex_m::hal::soc::board::dma_requests()
}

impl Dma {
    /// Creates a new Cortex-M DMA provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl DmaBase for Dma {
    fn support(&self) -> DmaSupport {
        let controllers = dma_controllers();
        let requests = dma_requests();
        let mut caps = DmaCaps::empty();

        if !controllers.is_empty() {
            caps |= DmaCaps::ENUMERATE_CONTROLLERS;
        }

        if !requests.is_empty() {
            caps |= DmaCaps::ENUMERATE_REQUESTS;
        }

        if caps.is_empty() {
            DmaSupport::unsupported()
        } else {
            DmaSupport {
                caps,
                implementation: DmaImplementationKind::Native,
            }
        }
    }
}

impl DmaCatalog for Dma {
    fn controllers(&self) -> &'static [DmaControllerDescriptor] {
        dma_controllers()
    }

    fn requests(&self) -> &'static [DmaRequestDescriptor] {
        dma_requests()
    }
}
