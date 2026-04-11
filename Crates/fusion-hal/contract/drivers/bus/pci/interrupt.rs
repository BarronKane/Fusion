//! PCI interrupt vocabulary.
//!
//! The MSI-X profile is intentionally minimal in the first cut. Table size and mask state are
//! enough for capability inspection; BAR index / table offset / PBA offset can be added when the
//! first real backend needs to program MSI-X instead of merely describe it.

/// Conventional PCI INTx pin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PciInterruptPin {
    IntA,
    IntB,
    IntC,
    IntD,
}

impl PciInterruptPin {
    #[must_use]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::IntA),
            2 => Some(Self::IntB),
            3 => Some(Self::IntC),
            4 => Some(Self::IntD),
            _ => None,
        }
    }
}

/// MSI capability truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PciMsiProfile {
    pub vector_count: u16,
    pub is_64_bit: bool,
    pub per_vector_masking: bool,
}

/// MSI-X capability truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PciMsixProfile {
    pub table_size: u16,
    pub masked: bool,
}

/// Interrupt-signaling truth for one function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PciInterruptProfile {
    pub legacy_pin: Option<PciInterruptPin>,
    pub msi: Option<PciMsiProfile>,
    pub msix: Option<PciMsixProfile>,
}

/// Interrupt lane for one PCI function.
pub trait PciInterruptContract {
    /// Returns one truthful interrupt-capability snapshot.
    fn interrupt_profile(&self) -> PciInterruptProfile;
}
