//! Shared PCI class-taxonomy helpers.

use super::core::*;

/// Known base-class taxonomy for one PCI class code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PciBaseClass {
    Unclassified,
    MassStorage,
    Network,
    Display,
    Multimedia,
    Memory,
    Bridge,
    Communication,
    GenericSystemPeripheral,
    Input,
    DockingStation,
    Processor,
    SerialBus,
    Wireless,
    IntelligentIo,
    SatelliteCommunication,
    Encryption,
    SignalProcessing,
    ProcessingAccelerator,
    NonEssentialInstrumentation,
    Coprocessor,
    VendorSpecific,
    Other(u8),
}

impl PciBaseClass {
    #[must_use]
    pub const fn from_u8(value: u8) -> Self {
        match value {
            0x00 => Self::Unclassified,
            0x01 => Self::MassStorage,
            0x02 => Self::Network,
            0x03 => Self::Display,
            0x04 => Self::Multimedia,
            0x05 => Self::Memory,
            0x06 => Self::Bridge,
            0x07 => Self::Communication,
            0x08 => Self::GenericSystemPeripheral,
            0x09 => Self::Input,
            0x0A => Self::DockingStation,
            0x0B => Self::Processor,
            0x0C => Self::SerialBus,
            0x0D => Self::Wireless,
            0x0E => Self::IntelligentIo,
            0x0F => Self::SatelliteCommunication,
            0x10 => Self::Encryption,
            0x11 => Self::SignalProcessing,
            0x12 => Self::ProcessingAccelerator,
            0x13 => Self::NonEssentialInstrumentation,
            0x40 => Self::Coprocessor,
            0xFF => Self::VendorSpecific,
            other => Self::Other(other),
        }
    }
}

/// Returns the known base-class classification for one raw class code.
#[must_use]
pub const fn base_class(class_code: PciClassCode) -> PciBaseClass {
    PciBaseClass::from_u8(class_code.base)
}
