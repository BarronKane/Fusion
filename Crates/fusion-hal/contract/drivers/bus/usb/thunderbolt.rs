//! Thunderbolt capability/profile vocabulary layered above USB4/Type-C/PD.

use super::core::*;
use super::error::*;

/// Thunderbolt generation/profile family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThunderboltGeneration {
    Thunderbolt3Compatible,
    Thunderbolt4,
    Thunderbolt5,
    Unknown,
}

/// Thunderbolt capability/profile truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ThunderboltCapabilities {
    pub usb4_compatible: bool,
    pub pcie_tunneling: bool,
    pub displayport_tunneling: bool,
    pub dma_protection_required: bool,
    pub certification_profile: bool,
}

/// Thunderbolt metadata surfaced by the platform or runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ThunderboltMetadata {
    pub generation: Option<ThunderboltGeneration>,
    pub minimum_bidirectional_gbps: Option<u16>,
    pub bandwidth_boost_gbps: Option<u16>,
    pub capabilities: ThunderboltCapabilities,
}

/// Shared Thunderbolt compatibility/profile surface.
pub trait ThunderboltContract: UsbCoreContract {
    /// Returns the current Thunderbolt capability/profile metadata.
    fn thunderbolt_metadata(&self) -> ThunderboltMetadata;

    /// Returns whether Thunderbolt compatibility/profile mode is currently active.
    ///
    /// # Errors
    ///
    /// Returns an error when the platform cannot characterize the current state.
    fn thunderbolt_active(&self) -> Result<bool, UsbError>;
}
