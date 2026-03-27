use super::MemDomainSet;

/// Stable identifier for a topology node in the fusion-pal memory catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemTopologyNodeId(pub u32);

/// Stable identifier for a topology link in the fusion-pal memory catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemTopologyLinkId(pub u32);

/// Kind of node present in the fusion-pal topology model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemTopologyNodeKind {
    /// Whole-machine or whole-board root.
    Machine,
    /// Package, socket, or comparable multi-controller grouping.
    Package,
    /// NUMA-like locality domain.
    NumaNode,
    /// Distinct memory controller or fabric-attached memory island.
    MemoryController,
    /// Device or accelerator that owns or primarily accesses a memory region.
    Device,
    /// Board- or firmware-defined physical bank or carveout.
    BoardRegion,
}

/// Relationship described by a topology link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemTopologyLinkKind {
    /// Parent-child containment relationship.
    ParentChild,
    /// Locality or access path between peers.
    AccessPath,
    /// Coherency relationship between nodes.
    Coherency,
}

/// Topology node descriptor surfaced by a fusion-pal memory catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemTopologyNode {
    /// Stable node identifier.
    pub id: MemTopologyNodeId,
    /// Coarse node kind.
    pub kind: MemTopologyNodeKind,
    /// Parent node when the topology is hierarchical.
    pub parent: Option<MemTopologyNodeId>,
    /// Memory domains naturally associated with this node.
    pub domains: MemDomainSet,
}

/// Topology link descriptor surfaced by a fusion-pal memory catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemTopologyLink {
    /// Stable link identifier.
    pub id: MemTopologyLinkId,
    /// Source node for the relationship.
    pub from: MemTopologyNodeId,
    /// Target node for the relationship.
    pub to: MemTopologyNodeId,
    /// Coarse relationship type.
    pub kind: MemTopologyLinkKind,
    /// Relative access-cost score where lower is better.
    pub distance: u32,
    /// Optional nominal bandwidth hint in bytes per second.
    pub bandwidth_bytes_per_sec: Option<u64>,
}
