//! AML host-side integration traits.

use crate::aml::{
    AmlAccessWidth,
    AmlNamespaceNodeId,
    AmlResult,
};

/// OSPM personality surface visible to AML.
pub trait AmlOspmInterface {
    fn osi_supported(&self, interface: &str) -> bool;
    fn os_revision(&self) -> u64;
}

/// Host-side stall/sleep surface.
pub trait AmlSleepHost {
    ///
    /// # Errors
    ///
    /// Returns an error if the requested operation cannot be completed.
    fn stall_us(&self, microseconds: u32) -> AmlResult<()>;
    ///
    /// # Errors
    ///
    /// Returns an error if the requested operation cannot be completed.
    fn sleep_ms(&self, milliseconds: u32) -> AmlResult<()>;
}

/// Host-side notification sink.
pub trait AmlNotifySink {
    ///
    /// # Errors
    ///
    /// Returns an error if the requested operation cannot be completed.
    fn notify(&self, source: AmlNamespaceNodeId, value: u8) -> AmlResult<()>;
}

/// Optional direct system-memory access surface.
pub trait AmlSystemMemoryHost {
    ///
    /// # Errors
    ///
    /// Returns an error if the requested operation cannot be completed.
    fn read_system_memory(&self, address: u64, width: AmlAccessWidth) -> AmlResult<u64>;
    ///
    /// # Errors
    ///
    /// Returns an error if the requested operation cannot be completed.
    fn write_system_memory(&self, address: u64, width: AmlAccessWidth, value: u64)
    -> AmlResult<()>;
}

/// Optional direct system-I/O access surface.
pub trait AmlSystemIoHost {
    ///
    /// # Errors
    ///
    /// Returns an error if the requested operation cannot be completed.
    fn read_system_io(&self, port: u64, width: AmlAccessWidth) -> AmlResult<u64>;
    ///
    /// # Errors
    ///
    /// Returns an error if the requested operation cannot be completed.
    fn write_system_io(&self, port: u64, width: AmlAccessWidth, value: u64) -> AmlResult<()>;
}

/// Optional direct PCI configuration access surface.
pub trait AmlPciConfigHost {
    ///
    /// # Errors
    ///
    /// Returns an error if the requested operation cannot be completed.
    fn read_pci_config(&self, address: u64, width: AmlAccessWidth) -> AmlResult<u64>;
    ///
    /// # Errors
    ///
    /// Returns an error if the requested operation cannot be completed.
    fn write_pci_config(&self, address: u64, width: AmlAccessWidth, value: u64) -> AmlResult<()>;
}

/// Optional direct embedded-controller access surface.
pub trait AmlEmbeddedControllerHost {
    ///
    /// # Errors
    ///
    /// Returns an error if the requested operation cannot be completed.
    fn read_embedded_controller(&self, register: u8) -> AmlResult<u8>;
    ///
    /// # Errors
    ///
    /// Returns an error if the requested operation cannot be completed.
    fn write_embedded_controller(&self, register: u8, value: u8) -> AmlResult<()>;
}

/// Complete AML host envelope expected by the VM.
pub trait AmlHost: AmlNotifySink + AmlOspmInterface + AmlSleepHost {}

/// Host envelope required for opregion and field execution.
pub trait AmlRegionAccessHost:
    AmlEmbeddedControllerHost + AmlHost + AmlPciConfigHost + AmlSystemIoHost + AmlSystemMemoryHost
{
}

impl<T> AmlRegionAccessHost for T where
    T: AmlEmbeddedControllerHost
        + AmlHost
        + AmlPciConfigHost
        + AmlSystemIoHost
        + AmlSystemMemoryHost
{
}
