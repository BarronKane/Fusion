//! Shared USB framework and capability vocabulary.

/// Flexible USB-family specification revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UsbSpecRevision {
    pub major: u8,
    pub minor: u8,
    pub sub_minor: u8,
}

impl UsbSpecRevision {
    pub const USB_1_0: Self = Self::new(1, 0, 0);
    pub const USB_1_1: Self = Self::new(1, 1, 0);
    pub const USB_2_0: Self = Self::new(2, 0, 0);
    pub const USB_3_0: Self = Self::new(3, 0, 0);
    pub const USB_3_1: Self = Self::new(3, 1, 0);
    pub const USB_3_2: Self = Self::new(3, 2, 0);
    pub const USB4_1_0: Self = Self::new(4, 1, 0);
    pub const USB4_2_0: Self = Self::new(4, 2, 0);

    #[must_use]
    pub const fn new(major: u8, minor: u8, sub_minor: u8) -> Self {
        Self {
            major,
            minor,
            sub_minor,
        }
    }
}

/// Canonical USB speed families shared across ordinary endpoint framework work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UsbSpeed {
    Low,
    Full,
    High,
    Super,
    SuperPlus,
}

/// Shared speed-capability truth for a device, controller, or path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct UsbSpeedSupport {
    pub low_speed: bool,
    pub full_speed: bool,
    pub high_speed: bool,
    pub super_speed: bool,
    pub super_speed_plus: bool,
}

/// Shared framework-capability truth for one USB path or function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct UsbCoreCapabilities {
    pub control_transfer: bool,
    pub bulk_transfer: bool,
    pub interrupt_transfer: bool,
    pub isochronous_transfer: bool,
    pub bos_descriptor: bool,
    pub lpm: bool,
    pub streams: bool,
    pub otg: bool,
    pub composite: bool,
    pub self_powered: bool,
    pub remote_wakeup: bool,
}

/// Coarse implementation kind surfaced by one USB provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UsbImplementationKind {
    Unsupported,
    Software,
    Hardware,
    Composite,
}

/// Truthful capability summary for one USB provider surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UsbSupport {
    pub implementation: UsbImplementationKind,
    pub host_controller: bool,
    pub device_controller: bool,
    pub typec: bool,
    pub pd: bool,
    pub usb4: bool,
    pub thunderbolt: bool,
}

impl UsbSupport {
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            implementation: UsbImplementationKind::Unsupported,
            host_controller: false,
            device_controller: false,
            typec: false,
            pd: false,
            usb4: false,
            thunderbolt: false,
        }
    }

    #[must_use]
    pub const fn is_unsupported(self) -> bool {
        matches!(self.implementation, UsbImplementationKind::Unsupported)
    }

    #[must_use]
    pub const fn has_any_surface(self) -> bool {
        self.host_controller
            || self.device_controller
            || self.typec
            || self.pd
            || self.usb4
            || self.thunderbolt
    }
}

/// Shared metadata for one USB device, controller, port, or path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct UsbCoreMetadata {
    /// Revision the hardware or firmware explicitly declares.
    pub declared_revision: Option<UsbSpecRevision>,
    /// Minimum framework revision the runtime can honestly infer.
    pub observed_minimum_revision: Option<UsbSpecRevision>,
    /// Maximum framework revision the runtime can honestly infer.
    pub observed_maximum_revision: Option<UsbSpecRevision>,
    /// Shared speed support truth.
    pub supported_speeds: UsbSpeedSupport,
    /// Shared framework capability truth.
    pub capabilities: UsbCoreCapabilities,
    /// Whether this path is surfaced as USB4-capable.
    pub usb4_capable: bool,
}

/// Direction on one USB endpoint or transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UsbDirection {
    Out,
    In,
}

/// Transfer family for one endpoint or transfer request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UsbTransferType {
    Control,
    Isochronous,
    Bulk,
    Interrupt,
}

/// Standard USB descriptor type vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UsbDescriptorType {
    Device,
    Configuration,
    String,
    Interface,
    Endpoint,
    Bos,
    DeviceCapability,
    InterfaceAssociation,
    Hid,
    Report,
    Physical,
    Hub,
    SuperSpeedHub,
    BillboardCapability,
    Other(u8),
}

impl UsbDescriptorType {
    #[must_use]
    pub const fn from_u8(value: u8) -> Self {
        match value {
            0x01 => Self::Device,
            0x02 => Self::Configuration,
            0x03 => Self::String,
            0x04 => Self::Interface,
            0x05 => Self::Endpoint,
            0x0F => Self::Bos,
            0x10 => Self::DeviceCapability,
            0x0B => Self::InterfaceAssociation,
            0x21 => Self::Hid,
            0x22 => Self::Report,
            0x23 => Self::Physical,
            0x29 => Self::Hub,
            0x2A => Self::SuperSpeedHub,
            0x0D => Self::BillboardCapability,
            other => Self::Other(other),
        }
    }

    #[must_use]
    pub const fn as_u8(self) -> u8 {
        match self {
            Self::Device => 0x01,
            Self::Configuration => 0x02,
            Self::String => 0x03,
            Self::Interface => 0x04,
            Self::Endpoint => 0x05,
            Self::Bos => 0x0F,
            Self::DeviceCapability => 0x10,
            Self::InterfaceAssociation => 0x0B,
            Self::Hid => 0x21,
            Self::Report => 0x22,
            Self::Physical => 0x23,
            Self::Hub => 0x29,
            Self::SuperSpeedHub => 0x2A,
            Self::BillboardCapability => 0x0D,
            Self::Other(value) => value,
        }
    }
}

/// Canonical endpoint number on one function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UsbEndpointNumber(pub u8);

/// Canonical endpoint address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UsbEndpointAddress {
    pub number: UsbEndpointNumber,
    pub direction: UsbDirection,
}

impl UsbEndpointAddress {
    #[must_use]
    pub const fn from_u8(value: u8) -> Self {
        Self {
            number: UsbEndpointNumber(value & 0x0F),
            direction: if (value & 0x80) != 0 {
                UsbDirection::In
            } else {
                UsbDirection::Out
            },
        }
    }

    #[must_use]
    pub const fn as_u8(self) -> u8 {
        let direction = match self.direction {
            UsbDirection::Out => 0x00,
            UsbDirection::In => 0x80,
        };

        direction | (self.number.0 & 0x0F)
    }
}

/// Canonical setup request recipient.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UsbRequestRecipient {
    Device,
    Interface,
    Endpoint,
    Other,
}

/// Canonical setup request type family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UsbRequestKind {
    Standard,
    Class,
    Vendor,
    Reserved,
}

/// Canonical control-setup packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UsbSetupPacket {
    pub direction: UsbDirection,
    pub kind: UsbRequestKind,
    pub recipient: UsbRequestRecipient,
    pub request: u8,
    pub value: u16,
    pub index: u16,
    pub length: u16,
}

/// Common descriptor header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UsbDescriptorHeader {
    pub length: u8,
    pub descriptor_type: UsbDescriptorType,
}

/// Canonical USB device descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UsbDeviceDescriptor {
    pub usb_revision: UsbSpecRevision,
    pub device_class: u8,
    pub device_subclass: u8,
    pub device_protocol: u8,
    pub max_packet_size_ep0: u16,
    pub vendor_id: u16,
    pub product_id: u16,
    pub device_revision: u16,
    pub manufacturer_string_index: u8,
    pub product_string_index: u8,
    pub serial_number_string_index: u8,
    pub configuration_count: u8,
}

/// Canonical USB configuration descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UsbConfigurationDescriptor {
    pub total_length: u16,
    pub interface_count: u8,
    pub configuration_value: u8,
    pub configuration_string_index: u8,
    pub attributes: u8,
    /// Raw `bMaxPower` value from the descriptor.
    ///
    /// This is intentionally left unitless because the meaning depends on the USB framework
    /// revision: USB 2.x interprets it in 2 mA units while USB 3.x uses 8 mA units.
    pub max_power_raw: u8,
}

impl UsbConfigurationDescriptor {
    /// Returns the descriptor's power budget in milliamps for the requested framework revision.
    #[must_use]
    pub const fn max_power_ma_for(self, revision: UsbSpecRevision) -> u16 {
        let units_ma = if revision.major >= 3 { 8 } else { 2 };
        (self.max_power_raw as u16) * units_ma
    }
}

/// Canonical USB interface descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UsbInterfaceDescriptor {
    pub interface_number: u8,
    pub alternate_setting: u8,
    pub endpoint_count: u8,
    pub interface_class: u8,
    pub interface_subclass: u8,
    pub interface_protocol: u8,
    pub interface_string_index: u8,
}

/// Canonical USB endpoint descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UsbEndpointDescriptor {
    pub address: UsbEndpointAddress,
    pub transfer_type: UsbTransferType,
    pub max_packet_size: u16,
    pub interval: u8,
}

/// Canonical BOS descriptor header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UsbBosDescriptor {
    pub total_length: u16,
    pub capability_count: u8,
}

/// Canonical device-capability descriptor header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UsbDeviceCapabilityDescriptor {
    pub capability_type: u8,
    pub payload: &'static [u8],
}

/// Shared trait for USB-family providers exposing one core capability snapshot.
pub trait UsbCoreContract {
    /// Returns one truthful coarse support snapshot for this provider.
    fn usb_support(&self) -> UsbSupport;

    /// Returns one truthful core metadata snapshot for this path.
    fn usb_core_metadata(&self) -> UsbCoreMetadata;
}
