//! USB controller identity and capability vocabulary.

use super::core::*;

/// Canonical USB controller implementation family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UsbControllerKind {
    Xhci,
    Ehci,
    Ohci,
    Uhci,
    Dwc2,
    Dwc3,
    ChipIdea,
    Musb,
    VendorSpecific,
    Unknown,
}

/// Realized role for one controller surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UsbControllerRole {
    Host,
    Device,
    DualRole,
    Unknown,
}

/// Where one controller was discovered from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UsbControllerDiscoverySource {
    StaticSoc,
    BoardManifest,
    Acpi,
    Devicetree,
    Pci,
    Manual,
    Unknown,
}

/// Canonical PCI location for one USB controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UsbPciAddress {
    pub segment: u16,
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

/// Canonical MMIO window describing one USB controller register block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UsbMmioWindow {
    pub base: u64,
    pub length: u64,
}

/// Concrete transport/attachment truth for one controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UsbControllerAttachment {
    Pci(UsbPciAddress),
    Mmio(UsbMmioWindow),
    SocIntegrated,
    PlatformBus,
    Unknown,
}

/// Controller-identification truth surfaced by platform discovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct UsbControllerIdentity {
    pub vendor_id: Option<u16>,
    pub device_id: Option<u16>,
    pub revision_id: Option<u8>,
    pub programming_interface: Option<u8>,
}

/// Controller capability truth that sits below the shared USB framework layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct UsbControllerCapabilities {
    pub dma: bool,
    pub sixty_four_bit_addressing: bool,
    pub multiple_interrupters: bool,
    pub streams: bool,
    pub port_power_control: bool,
    pub companion_controllers: bool,
    pub runtime_power_management: bool,
}

/// Shared controller metadata surfaced by host/device controller implementations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UsbControllerMetadata {
    pub kind: UsbControllerKind,
    pub role: UsbControllerRole,
    pub discovery_source: UsbControllerDiscoverySource,
    pub attachment: UsbControllerAttachment,
    pub identity: UsbControllerIdentity,
    pub interrupt_vectors: Option<u16>,
    pub visible_ports: Option<u16>,
}

impl Default for UsbControllerMetadata {
    fn default() -> Self {
        Self {
            kind: UsbControllerKind::Unknown,
            role: UsbControllerRole::Unknown,
            discovery_source: UsbControllerDiscoverySource::Unknown,
            attachment: UsbControllerAttachment::Unknown,
            identity: UsbControllerIdentity::default(),
            interrupt_vectors: None,
            visible_ports: None,
        }
    }
}

/// Shared controller introspection surface implemented by host/device controller backends.
pub trait UsbControllerContract: UsbCoreContract {
    /// Returns the controller identity and placement metadata.
    fn controller_metadata(&self) -> UsbControllerMetadata;

    /// Returns controller capabilities that sit below ordinary USB framework law.
    fn controller_capabilities(&self) -> UsbControllerCapabilities;
}
