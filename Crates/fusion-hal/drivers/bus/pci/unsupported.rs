//! Unsupported hardware-facing PCI substrate used when no backend is selected.

use fusion_hal::contract::drivers::bus::pci::{
    PciBarDescriptor,
    PciBridgeWindow,
    PciCapabilityRecord,
    PciConfigOffset,
    PciControllerDescriptor,
    PciDmaProfile,
    PciError,
    PciErrorReportingProfile,
    PciExpressProfile,
    PciExtendedCapabilityRecord,
    PciFunctionAddress,
    PciFunctionIdentity,
    PciFunctionProfile,
    PciHotplugProfile,
    PciInterruptProfile,
    PciPowerProfile,
    PciRomDescriptor,
    PciSegmentDescriptor,
    PciSupport,
    PciTopologyProfile,
    PciVirtualizationProfile,
};

use crate::interface::contract::{
    PciHardware,
    PciHardwareFunction,
};

const UNSUPPORTED_CONTROLLER: PciControllerDescriptor = PciControllerDescriptor {
    id: "unsupported-pci",
    name: "Unsupported PCI",
};

/// Unsupported PCI hardware substrate.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedPciHardware;

/// Unsupported hardware-facing PCI function placeholder.
#[derive(Debug, Clone, Copy)]
pub struct UnsupportedPciHardwareFunction {
    address: PciFunctionAddress,
}

impl Default for UnsupportedPciHardwareFunction {
    fn default() -> Self {
        Self {
            address: PciFunctionAddress {
                segment: fusion_hal::contract::drivers::bus::pci::PciSegment(0),
                bus: fusion_hal::contract::drivers::bus::pci::PciBus(0),
                device: fusion_hal::contract::drivers::bus::pci::PciDevice(0),
                function: fusion_hal::contract::drivers::bus::pci::PciFunction(0),
            },
        }
    }
}

impl PciHardware for UnsupportedPciHardware {
    type Function = UnsupportedPciHardwareFunction;

    fn provider_count() -> u8 {
        0
    }

    fn controller(_provider: u8) -> Option<&'static PciControllerDescriptor> {
        None
    }

    fn support(_provider: u8) -> PciSupport {
        PciSupport::unsupported()
    }

    fn segments(_provider: u8) -> &'static [PciSegmentDescriptor] {
        &[]
    }

    fn enumerate_functions(
        _provider: u8,
        _out: &mut [PciFunctionAddress],
    ) -> Result<usize, PciError> {
        Ok(0)
    }

    fn function(
        _provider: u8,
        address: PciFunctionAddress,
    ) -> Result<Option<Self::Function>, PciError> {
        Ok(Some(UnsupportedPciHardwareFunction { address }))
    }
}

impl PciHardwareFunction for UnsupportedPciHardwareFunction {
    fn address(&self) -> PciFunctionAddress {
        self.address
    }

    fn identity(&self) -> PciFunctionIdentity {
        PciFunctionIdentity {
            vendor_id: fusion_hal::contract::drivers::bus::pci::PciVendorId(0xffff),
            device_id: fusion_hal::contract::drivers::bus::pci::PciDeviceId(0xffff),
            subsystem_vendor_id: None,
            subsystem_id: None,
            class_code: fusion_hal::contract::drivers::bus::pci::PciClassCode {
                base: 0xff,
                sub: 0xff,
                interface: 0xff,
            },
            revision_id: 0xff,
        }
    }

    fn profile(&self) -> PciFunctionProfile {
        PciFunctionProfile {
            transport_family: fusion_hal::contract::drivers::bus::pci::PciTransportFamily::Other(
                "unsupported",
            ),
            configuration_model:
                fusion_hal::contract::drivers::bus::pci::PciConfigurationModel::Conventional256B,
            header_type: fusion_hal::contract::drivers::bus::pci::PciHeaderType::Other(0xff),
            multifunction: false,
            kind: fusion_hal::contract::drivers::bus::pci::PciFunctionKind::Unknown,
        }
    }

    fn bars(&self) -> &[PciBarDescriptor] {
        &[]
    }

    fn bridge_windows(&self) -> &[PciBridgeWindow] {
        &[]
    }

    fn option_rom(&self) -> Option<PciRomDescriptor> {
        None
    }

    fn capabilities(&self) -> &[PciCapabilityRecord] {
        &[]
    }

    fn extended_capabilities(&self) -> &[PciExtendedCapabilityRecord] {
        &[]
    }

    fn topology_profile(&self) -> PciTopologyProfile {
        PciTopologyProfile::default()
    }

    fn interrupt_profile(&self) -> PciInterruptProfile {
        PciInterruptProfile::default()
    }

    fn dma_profile(&self) -> PciDmaProfile {
        PciDmaProfile::default()
    }

    fn power_profile(&self) -> PciPowerProfile {
        PciPowerProfile::default()
    }

    fn error_reporting_profile(&self) -> PciErrorReportingProfile {
        PciErrorReportingProfile::default()
    }

    fn virtualization_profile(&self) -> PciVirtualizationProfile {
        PciVirtualizationProfile::default()
    }

    fn hotplug_profile(&self) -> Option<PciHotplugProfile> {
        None
    }

    fn pcie_profile(&self) -> Option<PciExpressProfile> {
        None
    }

    fn read_config_u8(&self, _offset: PciConfigOffset) -> Result<u8, PciError> {
        Err(PciError::unsupported())
    }

    fn read_config_u16(&self, _offset: PciConfigOffset) -> Result<u16, PciError> {
        Err(PciError::unsupported())
    }

    fn read_config_u32(&self, _offset: PciConfigOffset) -> Result<u32, PciError> {
        Err(PciError::unsupported())
    }

    fn write_config_u8(&mut self, _offset: PciConfigOffset, _value: u8) -> Result<(), PciError> {
        Err(PciError::unsupported())
    }

    fn write_config_u16(&mut self, _offset: PciConfigOffset, _value: u16) -> Result<(), PciError> {
        Err(PciError::unsupported())
    }

    fn write_config_u32(&mut self, _offset: PciConfigOffset, _value: u32) -> Result<(), PciError> {
        Err(PciError::unsupported())
    }
}

#[allow(dead_code)]
const _: &PciControllerDescriptor = &UNSUPPORTED_CONTROLLER;
