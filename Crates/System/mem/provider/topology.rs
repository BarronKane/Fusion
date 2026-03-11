use crate::mem::resource::MemoryDomainSet;

/// Stable identifier for a topology node in the provider's locality model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryTopologyNodeId(pub u32);

/// Stable identifier for a topology link in the provider's locality model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryTopologyLinkId(pub u32);

/// Borrowed topology view for provider-known locality information.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryTopology<'a> {
    /// Topology nodes such as NUMA-like regions, devices, or board-local memory banks.
    pub nodes: &'a [MemoryTopologyNode],
    /// Directed or undirected links relating those nodes.
    pub links: &'a [MemoryTopologyLink],
}

/// Kind of node present in a provider topology model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryTopologyNodeKind {
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
pub enum MemoryTopologyLinkKind {
    /// Parent-child containment relationship.
    ParentChild,
    /// Locality or access path between peers.
    AccessPath,
    /// Coherency relationship between nodes.
    Coherency,
}

/// Topology node descriptor surfaced by a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryTopologyNode {
    /// Stable node identifier.
    pub id: MemoryTopologyNodeId,
    /// Coarse node kind.
    pub kind: MemoryTopologyNodeKind,
    /// Parent node when the topology is hierarchical.
    pub parent: Option<MemoryTopologyNodeId>,
    /// Memory domains naturally associated with this node.
    pub domains: MemoryDomainSet,
}

/// Topology link descriptor surfaced by a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryTopologyLink {
    /// Stable link identifier.
    pub id: MemoryTopologyLinkId,
    /// Source node for the relationship.
    pub from: MemoryTopologyNodeId,
    /// Target node for the relationship.
    pub to: MemoryTopologyNodeId,
    /// Coarse relationship type.
    pub kind: MemoryTopologyLinkKind,
    /// Relative access-cost score where lower is better.
    pub distance: u32,
    /// Optional nominal bandwidth hint in bytes per second.
    pub bandwidth_bytes_per_sec: Option<u64>,
}

/// Topology request policy for a pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryTopologyPreference {
    /// No topology preference or requirement is imposed.
    Anywhere,
    /// Prefer a node when the provider can satisfy it, but permit degradation.
    PreferNode(MemoryTopologyNodeId),
    /// Require the chosen class, resource, or strategy to land on the given node.
    RequireNode(MemoryTopologyNodeId),
}
