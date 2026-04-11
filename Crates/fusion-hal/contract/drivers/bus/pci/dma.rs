//! PCI DMA and translation vocabulary.

/// DMA addressing-width truth for one function/path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PciDmaAddressWidth {
    Bits32,
    Bits64,
    Other(u8),
}

/// DMA and translation truth for one function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PciDmaProfile {
    pub bus_master_capable: bool,
    pub dma_address_width: Option<PciDmaAddressWidth>,
    pub cache_coherent: Option<bool>,
    pub ats: bool,
    pub pri: bool,
    pub pasid: bool,
    pub acs: bool,
}

/// DMA lane for one PCI function.
pub trait PciDmaContract {
    /// Returns one truthful DMA capability snapshot.
    fn dma_profile(&self) -> PciDmaProfile;
}
