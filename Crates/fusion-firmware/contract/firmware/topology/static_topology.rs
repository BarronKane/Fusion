//! Statically declared topology source contracts.

/// Degree of truthful static topology support.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StaticTopologySupport {
    Unsupported,
    Declared,
}

/// Firmware-topology contract for statically declared machine graphs.
pub trait StaticTopologyContract {
    fn static_topology_support(&self) -> StaticTopologySupport;
}
