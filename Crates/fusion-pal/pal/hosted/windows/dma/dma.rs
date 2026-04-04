//! Windows fusion-pal DMA backend.

use crate::contract::pal::dma::DmaControllerDescriptor;
use crate::contract::pal::dma::DmaRequestDescriptor;
use crate::contract::pal::dma::UnsupportedDma;

/// Selected Windows DMA provider type.
pub type PlatformDma = UnsupportedDma;

/// Returns the selected Windows DMA provider.
#[must_use]
pub const fn system_dma() -> PlatformDma {
    PlatformDma::new()
}

/// Returns the surfaced DMA controllers for the selected backend.
#[must_use]
pub fn dma_controllers() -> &'static [DmaControllerDescriptor] {
    &[]
}

/// Returns the surfaced DMA request lines for the selected backend.
#[must_use]
pub fn dma_requests() -> &'static [DmaRequestDescriptor] {
    &[]
}
