//! Shared PCI and config-space vocabulary.
//!
//! This lane owns the nouns every PCI consumer should be able to rely on without dragging in
//! platform discovery or PCIe-specific link theology. It also deliberately avoids one fake global
//! `PciVersion`: transport family, configuration model, PCIe capability version, link generation,
//! and optional capabilities are different axes and stay different here.

use super::error::*;

/// Coarse implementation kind surfaced by one PCI provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PciImplementationKind {
    Unsupported,
    Software,
    Hardware,
    Composite,
}

/// Truthful capability summary for one PCI provider surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PciSupport {
    pub implementation: PciImplementationKind,
    pub pcie: bool,
    pub interrupts: bool,
    pub dma: bool,
    pub power_management: bool,
    pub error_reporting: bool,
    pub virtualization: bool,
    pub hotplug: bool,
}

impl PciSupport {
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            implementation: PciImplementationKind::Unsupported,
            pcie: false,
            interrupts: false,
            dma: false,
            power_management: false,
            error_reporting: false,
            virtualization: false,
            hotplug: false,
        }
    }

    #[must_use]
    pub const fn is_unsupported(self) -> bool {
        matches!(self.implementation, PciImplementationKind::Unsupported)
    }

    #[must_use]
    pub const fn has_any_surface(self) -> bool {
        self.pcie
            || self.interrupts
            || self.dma
            || self.power_management
            || self.error_reporting
            || self.virtualization
            || self.hotplug
    }
}

/// Canonical PCI transport family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PciTransportFamily {
    ConventionalPci,
    PciExpress,
    PciX,
    Other(&'static str),
}

/// Configuration-space model visible for one function/path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PciConfigurationModel {
    Conventional256B,
    Enhanced4KiB,
}

/// PCI segment-group number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PciSegment(pub u16);

/// PCI bus number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PciBus(pub u8);

/// PCI device/slot number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PciDevice(u8);

impl PciDevice {
    /// Maximum representable PCI device/slot number.
    pub const MAX: u8 = 31;

    /// Builds one PCI device/slot number when the raw value fits the 5-bit field.
    #[must_use]
    pub const fn from_u8(value: u8) -> Option<Self> {
        if value <= Self::MAX {
            Some(Self(value))
        } else {
            None
        }
    }

    /// Returns the raw slot number.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// PCI function number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PciFunction(u8);

impl PciFunction {
    /// Maximum representable PCI function number.
    pub const MAX: u8 = 7;

    /// Builds one PCI function number when the raw value fits the 3-bit field.
    #[must_use]
    pub const fn from_u8(value: u8) -> Option<Self> {
        if value <= Self::MAX {
            Some(Self(value))
        } else {
            None
        }
    }

    /// Returns the raw function number.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// Conventional Requester ID within one segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PciRequesterId(pub u16);

/// Canonical PCI function address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PciFunctionAddress {
    pub segment: PciSegment,
    pub bus: PciBus,
    pub device: PciDevice,
    pub function: PciFunction,
}

impl PciFunctionAddress {
    /// Returns the Requester ID portion for this address.
    #[must_use]
    pub const fn requester_id(self) -> PciRequesterId {
        PciRequesterId(
            ((self.bus.0 as u16) << 8)
                | ((self.device.get() as u16) << 3)
                | (self.function.get() as u16),
        )
    }
}

/// Configuration-space byte offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PciConfigOffset(pub u16);

/// Vendor ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PciVendorId(pub u16);

/// Device ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PciDeviceId(pub u16);

/// Subsystem vendor ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PciSubsystemVendorId(pub u16);

/// Subsystem device ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PciSubsystemId(pub u16);

/// Raw PCI class-code tuple.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PciClassCode {
    pub base: u8,
    pub sub: u8,
    pub interface: u8,
}

/// Header-layout truth for one function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PciHeaderType {
    Type0,
    Type1,
    Type2,
    Other(u8),
}

impl PciHeaderType {
    #[must_use]
    pub const fn from_u8(value: u8) -> Self {
        match value & 0x7f {
            0x00 => Self::Type0,
            0x01 => Self::Type1,
            0x02 => Self::Type2,
            other => Self::Other(other),
        }
    }
}

/// Broad function kind or PCIe role truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PciFunctionKind {
    Endpoint,
    LegacyEndpoint,
    Bridge,
    RootPort,
    UpstreamSwitchPort,
    DownstreamSwitchPort,
    PcieToPciBridge,
    PciToPcieBridge,
    RootComplexIntegratedEndpoint,
    RootComplexEventCollector,
    CardBusBridge,
    Other(&'static str),
    /// Real backends are allowed to say "not yet classifiable" without lying.
    Unknown,
}

/// Human-meaningful identity for one PCI function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PciFunctionIdentity {
    pub vendor_id: PciVendorId,
    pub device_id: PciDeviceId,
    pub subsystem_vendor_id: Option<PciSubsystemVendorId>,
    pub subsystem_id: Option<PciSubsystemId>,
    pub class_code: PciClassCode,
    pub revision_id: u8,
}

/// Shared function-profile truth independent of optional PCIe lanes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PciFunctionProfile {
    pub transport_family: PciTransportFamily,
    pub configuration_model: PciConfigurationModel,
    pub header_type: PciHeaderType,
    pub multifunction: bool,
    pub kind: PciFunctionKind,
}

/// BAR family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PciBarKind {
    Io,
    Memory32,
    Memory64,
}

/// One decoded BAR window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PciBarDescriptor {
    pub index: u8,
    pub kind: PciBarKind,
    pub base: u64,
    pub size: u64,
    pub prefetchable: bool,
    pub implemented: bool,
}

/// Optional ROM BAR truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PciRomDescriptor {
    pub base: u64,
    pub size: u64,
    pub enabled: bool,
}

/// Bridge resource-window family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PciBridgeWindowKind {
    Io,
    Memory32,
    Memory64,
}

/// One decoded bridge window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PciBridgeWindow {
    pub kind: PciBridgeWindowKind,
    pub base: u64,
    pub limit: u64,
    pub prefetchable: bool,
}

/// Standard PCI capability IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PciCapabilityId {
    PowerManagement,
    Agp,
    VitalProductData,
    SlotIdentification,
    Msi,
    CompactPciHotSwap,
    PciX,
    HyperTransport,
    VendorSpecific,
    DebugPort,
    CompactPciResourceControl,
    HotPlugController,
    BridgeSubsystemVendor,
    Agp8x,
    SecureDevice,
    PciExpress,
    Msix,
    SataDataIndexConf,
    AdvancedFeatures,
    EnhancedAllocation,
    FlatteningPortalBridge,
    /// Unknown/unsupported capability ids must survive discovery intact.
    Other(u8),
}

impl PciCapabilityId {
    #[must_use]
    pub const fn from_u8(value: u8) -> Self {
        match value {
            0x01 => Self::PowerManagement,
            0x02 => Self::Agp,
            0x03 => Self::VitalProductData,
            0x04 => Self::SlotIdentification,
            0x05 => Self::Msi,
            0x06 => Self::CompactPciHotSwap,
            0x07 => Self::PciX,
            0x08 => Self::HyperTransport,
            0x09 => Self::VendorSpecific,
            0x0A => Self::DebugPort,
            0x0B => Self::CompactPciResourceControl,
            0x0C => Self::HotPlugController,
            0x0D => Self::BridgeSubsystemVendor,
            0x0E => Self::Agp8x,
            0x0F => Self::SecureDevice,
            0x10 => Self::PciExpress,
            0x11 => Self::Msix,
            0x12 => Self::SataDataIndexConf,
            0x13 => Self::AdvancedFeatures,
            0x14 => Self::EnhancedAllocation,
            0x15 => Self::FlatteningPortalBridge,
            other => Self::Other(other),
        }
    }

    #[must_use]
    pub const fn as_u8(self) -> u8 {
        match self {
            Self::PowerManagement => 0x01,
            Self::Agp => 0x02,
            Self::VitalProductData => 0x03,
            Self::SlotIdentification => 0x04,
            Self::Msi => 0x05,
            Self::CompactPciHotSwap => 0x06,
            Self::PciX => 0x07,
            Self::HyperTransport => 0x08,
            Self::VendorSpecific => 0x09,
            Self::DebugPort => 0x0A,
            Self::CompactPciResourceControl => 0x0B,
            Self::HotPlugController => 0x0C,
            Self::BridgeSubsystemVendor => 0x0D,
            Self::Agp8x => 0x0E,
            Self::SecureDevice => 0x0F,
            Self::PciExpress => 0x10,
            Self::Msix => 0x11,
            Self::SataDataIndexConf => 0x12,
            Self::AdvancedFeatures => 0x13,
            Self::EnhancedAllocation => 0x14,
            Self::FlatteningPortalBridge => 0x15,
            Self::Other(value) => value,
        }
    }
}

/// One standard capability record from the linked-list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PciCapabilityRecord {
    pub id: PciCapabilityId,
    pub offset: PciConfigOffset,
    pub next: Option<PciConfigOffset>,
}

/// Extended PCIe capability IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PciExtendedCapabilityId {
    AdvancedErrorReporting,
    VirtualChannel,
    DeviceSerialNumber,
    PowerBudgeting,
    VendorSpecific,
    AccessControlServices,
    AlternativeRoutingIdInterpretation,
    AddressTranslationServices,
    SingleRootIoVirtualization,
    MultiRootIoVirtualization,
    PageRequestInterface,
    ResizableBar,
    ProcessAddressSpaceId,
    DownstreamPortContainment,
    DataObjectExchange,
    /// Unknown/unsupported extended capability ids must survive discovery intact.
    Other(u16),
}

impl PciExtendedCapabilityId {
    #[must_use]
    pub const fn from_u16(value: u16) -> Self {
        match value {
            0x0001 => Self::AdvancedErrorReporting,
            0x0002 => Self::VirtualChannel,
            0x0003 => Self::DeviceSerialNumber,
            0x0004 => Self::PowerBudgeting,
            0x000B => Self::VendorSpecific,
            0x000D => Self::AccessControlServices,
            0x000E => Self::AlternativeRoutingIdInterpretation,
            0x000F => Self::AddressTranslationServices,
            0x0010 => Self::SingleRootIoVirtualization,
            0x0011 => Self::MultiRootIoVirtualization,
            0x0013 => Self::PageRequestInterface,
            0x0015 => Self::ResizableBar,
            0x001B => Self::ProcessAddressSpaceId,
            0x001D => Self::DownstreamPortContainment,
            0x002E => Self::DataObjectExchange,
            other => Self::Other(other),
        }
    }

    #[must_use]
    pub const fn as_u16(self) -> u16 {
        match self {
            Self::AdvancedErrorReporting => 0x0001,
            Self::VirtualChannel => 0x0002,
            Self::DeviceSerialNumber => 0x0003,
            Self::PowerBudgeting => 0x0004,
            Self::VendorSpecific => 0x000B,
            Self::AccessControlServices => 0x000D,
            Self::AlternativeRoutingIdInterpretation => 0x000E,
            Self::AddressTranslationServices => 0x000F,
            Self::SingleRootIoVirtualization => 0x0010,
            Self::MultiRootIoVirtualization => 0x0011,
            Self::PageRequestInterface => 0x0013,
            Self::ResizableBar => 0x0015,
            Self::ProcessAddressSpaceId => 0x001B,
            Self::DownstreamPortContainment => 0x001D,
            Self::DataObjectExchange => 0x002E,
            Self::Other(value) => value,
        }
    }
}

/// One extended capability record from enhanced config space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PciExtendedCapabilityRecord {
    pub id: PciExtendedCapabilityId,
    pub version: u8,
    pub offset: PciConfigOffset,
    pub next: Option<PciConfigOffset>,
}

/// Core public contract for one PCI function.
pub trait PciFunctionContract {
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

    /// Reads one byte from configuration space.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the offset is invalid or unreadable.
    fn read_config_u8(&self, offset: PciConfigOffset) -> Result<u8, PciError>;

    /// Reads one 16-bit word from configuration space.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the offset is invalid or unreadable.
    fn read_config_u16(&self, offset: PciConfigOffset) -> Result<u16, PciError>;

    /// Reads one 32-bit dword from configuration space.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the offset is invalid or unreadable.
    fn read_config_u32(&self, offset: PciConfigOffset) -> Result<u32, PciError>;

    /// Writes one byte into configuration space.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the offset is invalid or the write is disallowed.
    fn write_config_u8(&mut self, offset: PciConfigOffset, value: u8) -> Result<(), PciError>;

    /// Writes one 16-bit word into configuration space.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the offset is invalid or the write is disallowed.
    fn write_config_u16(&mut self, offset: PciConfigOffset, value: u16) -> Result<(), PciError>;

    /// Writes one 32-bit dword into configuration space.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the offset is invalid or the write is disallowed.
    fn write_config_u32(&mut self, offset: PciConfigOffset, value: u32) -> Result<(), PciError>;
}
