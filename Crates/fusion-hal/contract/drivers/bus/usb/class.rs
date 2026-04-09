//! USB class vocabulary layered above USB framework law.

use super::core::*;

/// Canonical USB standard class-code family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UsbStandardClass {
    PerInterface,
    Audio,
    Communications,
    Hid,
    Physical,
    Imaging,
    Printer,
    MassStorage,
    Hub,
    CdcData,
    SmartCard,
    ContentSecurity,
    Video,
    PersonalHealthcare,
    AudioVideo,
    Billboard,
    Diagnostic,
    WirelessController,
    Miscellaneous,
    ApplicationSpecific,
    VendorSpecific,
    Unknown(u8),
}

impl UsbStandardClass {
    #[must_use]
    pub const fn from_u8(value: u8) -> Self {
        match value {
            0x00 => Self::PerInterface,
            0x01 => Self::Audio,
            0x02 => Self::Communications,
            0x03 => Self::Hid,
            0x05 => Self::Physical,
            0x06 => Self::Imaging,
            0x07 => Self::Printer,
            0x08 => Self::MassStorage,
            0x09 => Self::Hub,
            0x0A => Self::CdcData,
            0x0B => Self::SmartCard,
            0x0D => Self::ContentSecurity,
            0x0E => Self::Video,
            0x0F => Self::PersonalHealthcare,
            0x10 => Self::AudioVideo,
            0x11 => Self::Billboard,
            0xDC => Self::Diagnostic,
            0xE0 => Self::WirelessController,
            0xEF => Self::Miscellaneous,
            0xFE => Self::ApplicationSpecific,
            0xFF => Self::VendorSpecific,
            other => Self::Unknown(other),
        }
    }
}

/// Fully qualified USB class identity triplet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UsbClassIdentity {
    pub standard_class: UsbStandardClass,
    pub class_code: u8,
    pub subclass: u8,
    pub protocol: u8,
}

impl UsbClassIdentity {
    #[must_use]
    pub const fn new(class_code: u8, subclass: u8, protocol: u8) -> Self {
        Self {
            standard_class: UsbStandardClass::from_u8(class_code),
            class_code,
            subclass,
            protocol,
        }
    }
}

/// Shared class-bound capability surface for one USB function or interface.
pub trait UsbClassContract: UsbCoreContract {
    /// Returns the bound class identity for this function or interface.
    fn class_identity(&self) -> UsbClassIdentity;
}
