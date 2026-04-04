//! Backend-neutral DMA capability and catalog vocabulary.

mod caps;
mod unsupported;

pub use caps::*;
pub use unsupported::*;

/// Capability trait for DMA-capable PAL backends.
pub trait DmaBase {
    /// Reports the truthful DMA capability surface for this backend.
    fn support(&self) -> DmaSupport;
}

/// Catalog contract for DMA-capable PAL backends.
pub trait DmaCatalog: DmaBase {
    /// Returns the surfaced DMA controllers for this backend.
    #[must_use]
    fn controllers(&self) -> &'static [DmaControllerDescriptor];

    /// Returns the surfaced DMA request lines for this backend.
    #[must_use]
    fn requests(&self) -> &'static [DmaRequestDescriptor];
}
