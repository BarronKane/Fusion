//! Hardware-facing PCI substrate contract consumed by the universal PCI driver.

use fusion_hal::contract::drivers::bus::pci::{
    PciBridgeWindow,
    PciCapabilityRecord,
    PciControllerDescriptor,
    PciDmaContract,
    PciDmaProfile,
    PciError,
    PciErrorReportingContract,
    PciErrorReportingProfile,
    PciExpressContract,
    PciExpressProfile,
    PciExtendedCapabilityRecord,
    PciFunctionAddress,
    PciFunctionContract,
    PciFunctionIdentity,
    PciFunctionProfile,
    PciHotplugContract,
    PciHotplugProfile,
    PciInterruptContract,
    PciInterruptProfile,
    PciPowerContract,
    PciPowerProfile,
    PciRomDescriptor,
    PciSegmentDescriptor,
    PciSupport,
    PciTopologyContract,
    PciTopologyProfile,
    PciVirtualizationContract,
    PciVirtualizationProfile,
    PciConfigOffset,
    PciBarDescriptor,
};

/// Hardware-facing contract for one PCI controller substrate family.
pub trait PciHardware {
    /// Concrete function handle surfaced by this hardware substrate.
    type Function: PciHardwareFunction;

    /// Returns the number of surfaced controller/providers.
    fn provider_count() -> u8;

    /// Returns the selected controller descriptor.
    fn controller(provider: u8) -> Option<&'static PciControllerDescriptor>;

    /// Returns the truthful coarse support summary for this provider.
    fn support(provider: u8) -> PciSupport;

    /// Returns the segment/bus ranges surfaced by this provider.
    fn segments(provider: u8) -> &'static [PciSegmentDescriptor];

    /// Enumerates visible functions through this provider.
    ///
    /// # Errors
    ///
    /// Returns one honest error when enumeration fails.
    fn enumerate_functions(provider: u8, out: &mut [PciFunctionAddress])
    -> Result<usize, PciError>;

    /// Opens one hardware-facing function handle when visible.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the function cannot be reached.
    fn function(
        provider: u8,
        address: PciFunctionAddress,
    ) -> Result<Option<Self::Function>, PciError>;
}

/// Hardware-facing contract for one PCI function handle.
pub trait PciHardwareFunction {
    /// Returns the canonical address of this function.
    fn address(&self) -> PciFunctionAddress;

    /// Returns the marketed identity tuple for this function.
    fn identity(&self) -> PciFunctionIdentity;

    /// Returns the coarse transport/header/function profile.
    fn profile(&self) -> PciFunctionProfile;

    /// Returns the decoded BARs visible for this function.
    fn bars(&self) -> &[PciBarDescriptor];

    /// Returns the decoded bridge windows visible for this function.
    fn bridge_windows(&self) -> &[PciBridgeWindow];

    /// Returns the decoded option-ROM BAR, when one exists.
    fn option_rom(&self) -> Option<PciRomDescriptor>;

    /// Returns the walked standard capability records.
    fn capabilities(&self) -> &[PciCapabilityRecord];

    /// Returns the walked extended capability records.
    fn extended_capabilities(&self) -> &[PciExtendedCapabilityRecord];

    /// Returns one truthful topology relationship snapshot.
    fn topology_profile(&self) -> PciTopologyProfile;

    /// Returns one truthful interrupt-capability snapshot.
    fn interrupt_profile(&self) -> PciInterruptProfile;

    /// Returns one truthful DMA capability snapshot.
    fn dma_profile(&self) -> PciDmaProfile;

    /// Returns one truthful power-management snapshot.
    fn power_profile(&self) -> PciPowerProfile;

    /// Returns one truthful error-reporting capability snapshot.
    fn error_reporting_profile(&self) -> PciErrorReportingProfile;

    /// Returns one truthful virtualization capability snapshot.
    fn virtualization_profile(&self) -> PciVirtualizationProfile;

    /// Returns one truthful hot-plug / slot snapshot when available.
    fn hotplug_profile(&self) -> Option<PciHotplugProfile>;

    /// Returns one truthful PCIe profile snapshot when this function participates in PCIe.
    fn pcie_profile(&self) -> Option<PciExpressProfile>;

    /// Reads one byte from configuration space.
    fn read_config_u8(&self, offset: PciConfigOffset) -> Result<u8, PciError>;

    /// Reads one 16-bit word from configuration space.
    fn read_config_u16(&self, offset: PciConfigOffset) -> Result<u16, PciError>;

    /// Reads one 32-bit dword from configuration space.
    fn read_config_u32(&self, offset: PciConfigOffset) -> Result<u32, PciError>;

    /// Writes one byte into configuration space.
    fn write_config_u8(&mut self, offset: PciConfigOffset, value: u8) -> Result<(), PciError>;

    /// Writes one 16-bit word into configuration space.
    fn write_config_u16(&mut self, offset: PciConfigOffset, value: u16) -> Result<(), PciError>;

    /// Writes one 32-bit dword into configuration space.
    fn write_config_u32(&mut self, offset: PciConfigOffset, value: u32) -> Result<(), PciError>;
}

#[allow(dead_code)]
fn _trait_shape_check<T, F>()
where
    T: PciHardware<Function = F>,
    F: PciHardwareFunction,
    F: PciFunctionContract
        + PciExpressContract
        + PciTopologyContract
        + PciInterruptContract
        + PciDmaContract
        + PciPowerContract
        + PciErrorReportingContract
        + PciVirtualizationContract
        + PciHotplugContract,
{
    let _ = core::marker::PhantomData::<T>;
}
