//! PCI topology and hierarchy vocabulary.

use super::core::*;

/// Topology-adjacent relationship truth for one function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PciTopologyProfile {
    pub parent: Option<PciFunctionAddress>,
    pub secondary_bus: Option<PciBus>,
    pub subordinate_bus: Option<PciBus>,
    pub slot: Option<u8>,
}

/// Topology lane for one PCI function.
pub trait PciTopologyContract {
    /// Returns one truthful topology relationship snapshot for this function.
    fn topology_profile(&self) -> PciTopologyProfile;
}
