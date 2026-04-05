//! Backend-neutral unsupported DMA implementation.

use super::{
    DmaBaseContract,
    DmaControllerDescriptor,
    DmaCatalogContract,
    DmaRequestDescriptor,
    DmaSupport,
};

/// Unsupported DMA provider placeholder.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedDma;

impl UnsupportedDma {
    /// Creates a new unsupported DMA provider placeholder.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl DmaBaseContract for UnsupportedDma {
    fn support(&self) -> DmaSupport {
        DmaSupport::unsupported()
    }
}

impl DmaCatalogContract for UnsupportedDma {
    fn controllers(&self) -> &'static [DmaControllerDescriptor] {
        &[]
    }

    fn requests(&self) -> &'static [DmaRequestDescriptor] {
        &[]
    }
}
