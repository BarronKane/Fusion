//! PCI controller/provider vocabulary.

use super::core::*;
use super::dma::*;
use super::error::*;
use super::hotplug::*;
use super::interrupt::*;
use super::pcie::*;
use super::power::*;
use super::topology::*;
use super::virtualization::*;

/// Human-facing descriptor for one PCI controller/provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PciControllerDescriptor {
    pub id: &'static str,
    pub name: &'static str,
}

/// One bus-range segment surfaced by one PCI controller/provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PciSegmentDescriptor {
    pub segment: PciSegment,
    pub start_bus: PciBus,
    pub end_bus: PciBus,
}

/// Public controller surface for one selected PCI provider.
pub trait PciControllerContract {
    /// Concrete function handle type returned by this provider.
    type Function: PciFunctionContract
        + PciExpressContract
        + PciTopologyContract
        + PciInterruptContract
        + PciDmaContract
        + PciPowerContract
        + PciErrorReportingContract
        + PciVirtualizationContract
        + PciHotplugContract;

    /// Returns the descriptor for this selected controller/provider.
    fn controller(&self) -> &'static PciControllerDescriptor;

    /// Returns the truthful coarse support summary for this provider.
    fn support(&self) -> PciSupport;

    /// Returns the segment/bus ranges owned by this provider.
    fn segments(&self) -> &'static [PciSegmentDescriptor];

    /// Enumerates visible functions through this provider.
    ///
    /// # Errors
    ///
    /// Returns one honest error when enumeration fails.
    fn enumerate_functions(&self, out: &mut [PciFunctionAddress]) -> Result<usize, PciError>;

    /// Opens one function handle at the requested address when visible.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the function cannot be reached.
    fn function(&self, address: PciFunctionAddress) -> Result<Option<Self::Function>, PciError>;
}
