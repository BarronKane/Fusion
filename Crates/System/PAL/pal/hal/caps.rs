use bitflags::bitflags;

/// Shared authority bitset specialized for hardware support.
pub use crate::pal::caps::AuthoritySet as HardwareAuthoritySet;
/// Shared guarantee ladder specialized for hardware support.
pub use crate::pal::caps::Guarantee as HardwareGuarantee;
/// Shared implementation-category vocabulary specialized for hardware support.
pub use crate::pal::caps::ImplementationKind as HardwareImplementationKind;

bitflags! {
    /// CPU- and ABI-oriented hardware-query capabilities.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct HardwareCpuCaps: u32 {
        /// The provider can surface a CPU descriptor.
        const DESCRIPTOR       = 1 << 0;
        /// The provider can surface CPU vendor or manufacturer identity.
        const VENDOR           = 1 << 1;
        /// The provider can surface a cache-line size relevant to padding and false sharing.
        const CACHE_LINE_BYTES = 1 << 2;
        /// The provider can characterize the hardware memory-ordering model.
        const MEMORY_ORDERING  = 1 << 3;
        /// The provider can characterize native atomic-width support.
        const ATOMIC_WIDTHS    = 1 << 4;
        /// The provider can characterize the relevant stack ABI.
        const STACK_ABI        = 1 << 5;
        /// The provider can characterize runtime-usable SIMD/vector features.
        const SIMD             = 1 << 6;
    }
}

bitflags! {
    /// Topology-enumeration capabilities a hardware provider may expose.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct HardwareTopologyCaps: u32 {
        /// The provider can surface a coarse topology summary.
        const SUMMARY          = 1 << 0;
        /// The provider can enumerate logical CPU identifiers.
        const LOGICAL_CPUS     = 1 << 1;
        /// The provider can enumerate core identifiers.
        const CORES            = 1 << 2;
        /// The provider can enumerate cluster identifiers.
        const CLUSTERS         = 1 << 3;
        /// The provider can enumerate package or socket identifiers.
        const PACKAGES         = 1 << 4;
        /// The provider can enumerate NUMA-node identifiers.
        const NUMA_NODES       = 1 << 5;
        /// The provider can enumerate heterogeneous core-class identifiers.
        const CORE_CLASSES     = 1 << 6;
    }
}

/// CPU- and ABI-oriented hardware support surfaced by a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HardwareCpuSupport {
    /// Fine-grained CPU capability flags.
    pub caps: HardwareCpuCaps,
    /// Strength of the CPU-descriptor guarantee.
    pub descriptor: HardwareGuarantee,
    /// Strength of the vendor/manufacturer guarantee.
    pub vendor: HardwareGuarantee,
    /// Strength of the cache-line-size guarantee.
    pub cache_line_bytes: HardwareGuarantee,
    /// Strength of the memory-ordering guarantee.
    pub memory_ordering: HardwareGuarantee,
    /// Strength of the atomic-width guarantee.
    pub atomic_widths: HardwareGuarantee,
    /// Strength of the stack-ABI guarantee.
    pub stack_abi: HardwareGuarantee,
    /// Strength of the runtime SIMD/vector guarantee.
    pub simd: HardwareGuarantee,
    /// Evidence sources contributing to the CPU support record.
    pub authorities: HardwareAuthoritySet,
    /// Whether the support is native, emulated, or unavailable.
    pub implementation: HardwareImplementationKind,
}

impl HardwareCpuSupport {
    /// Returns an explicitly unsupported CPU support surface.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            caps: HardwareCpuCaps::empty(),
            descriptor: HardwareGuarantee::Unsupported,
            vendor: HardwareGuarantee::Unsupported,
            cache_line_bytes: HardwareGuarantee::Unsupported,
            memory_ordering: HardwareGuarantee::Unsupported,
            atomic_widths: HardwareGuarantee::Unsupported,
            stack_abi: HardwareGuarantee::Unsupported,
            simd: HardwareGuarantee::Unsupported,
            authorities: HardwareAuthoritySet::empty(),
            implementation: HardwareImplementationKind::Unsupported,
        }
    }
}

/// Topology-oriented hardware support surfaced by a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HardwareTopologySupport {
    /// Fine-grained topology capability flags.
    pub caps: HardwareTopologyCaps,
    /// Strength of the coarse topology-summary guarantee.
    pub summary: HardwareGuarantee,
    /// Strength of logical-CPU enumeration guarantees.
    pub logical_cpus: HardwareGuarantee,
    /// Strength of core enumeration guarantees.
    pub cores: HardwareGuarantee,
    /// Strength of cluster enumeration guarantees.
    pub clusters: HardwareGuarantee,
    /// Strength of package enumeration guarantees.
    pub packages: HardwareGuarantee,
    /// Strength of NUMA-node enumeration guarantees.
    pub numa_nodes: HardwareGuarantee,
    /// Strength of core-class enumeration guarantees.
    pub core_classes: HardwareGuarantee,
    /// Evidence sources contributing to the topology support record.
    pub authorities: HardwareAuthoritySet,
    /// Whether the support is native, emulated, or unavailable.
    pub implementation: HardwareImplementationKind,
}

impl HardwareTopologySupport {
    /// Returns an explicitly unsupported topology support surface.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            caps: HardwareTopologyCaps::empty(),
            summary: HardwareGuarantee::Unsupported,
            logical_cpus: HardwareGuarantee::Unsupported,
            cores: HardwareGuarantee::Unsupported,
            clusters: HardwareGuarantee::Unsupported,
            packages: HardwareGuarantee::Unsupported,
            numa_nodes: HardwareGuarantee::Unsupported,
            core_classes: HardwareGuarantee::Unsupported,
            authorities: HardwareAuthoritySet::empty(),
            implementation: HardwareImplementationKind::Unsupported,
        }
    }
}

/// Full hardware support surface for a selected provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HardwareSupport {
    /// CPU- and ABI-oriented support.
    pub cpu: HardwareCpuSupport,
    /// Topology-oriented support.
    pub topology: HardwareTopologySupport,
}

impl HardwareSupport {
    /// Returns a fully unsupported hardware support surface.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            cpu: HardwareCpuSupport::unsupported(),
            topology: HardwareTopologySupport::unsupported(),
        }
    }
}
