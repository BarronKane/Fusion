//! PCI power-management vocabulary.

/// PCI device power state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PciPowerState {
    D0,
    D1,
    D2,
    D3Hot,
    D3Cold,
}

/// PCI power-management and wake truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PciPowerProfile {
    pub capability_version: Option<u8>,
    pub current_state: Option<PciPowerState>,
    pub pme_supported: bool,
    pub pme_enabled: bool,
    pub aux_current_ma: Option<u16>,
    pub aspm_supported: bool,
    pub aspm_enabled: bool,
}

/// Power-management lane for one PCI function.
pub trait PciPowerContract {
    /// Returns one truthful power-management snapshot.
    fn power_profile(&self) -> PciPowerProfile;
}
