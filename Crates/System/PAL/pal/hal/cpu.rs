use bitflags::bitflags;

/// Stable identifier for a hardware-topology node shared across hardware and thread APIs.
///
/// This currently aliases the machine-topology node identifiers surfaced by the PAL memory
/// topology model. The name here is intentionally hardware-oriented so package and NUMA
/// placement do not have to masquerade as memory-only concepts in higher layers.
pub type HardwareTopologyNodeId = crate::pal::mem::MemTopologyNodeId;

/// Coarse CPU architecture identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HardwareCpuArchitecture {
    /// The provider cannot characterize the CPU architecture honestly.
    Unknown,
    /// 64-bit x86.
    X86_64,
    /// 64-bit Arm.
    Aarch64,
    /// 32-bit Arm.
    Arm,
    /// 64-bit RISC-V.
    RiscV64,
    /// Another architecture not yet modeled explicitly.
    Other,
}

/// Coarse CPU vendor or manufacturer identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HardwareCpuVendor {
    /// The provider cannot surface vendor identity honestly.
    Unknown,
    /// Intel.
    Intel,
    /// AMD.
    Amd,
    /// Arm Ltd. or Arm-branded silicon.
    Arm,
    /// Apple silicon.
    Apple,
    /// Qualcomm.
    Qualcomm,
    /// Ampere.
    Ampere,
    /// Broadcom.
    Broadcom,
    /// Microsoft-branded silicon.
    Microsoft,
    /// Another vendor not yet modeled explicitly.
    Other,
}

/// Endianness of the selected target execution model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HardwareEndian {
    /// The provider cannot characterize endianness honestly.
    Unknown,
    /// Little-endian execution model.
    Little,
    /// Big-endian execution model.
    Big,
}

/// Coarse hardware memory-ordering model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HardwareMemoryOrdering {
    /// The provider cannot characterize the memory-ordering model honestly.
    Unknown,
    /// Total-store-order style model, such as x86-64.
    TotalStoreOrder,
    /// A weaker, explicitly ordered model that requires stronger synchronization fences.
    WeaklyOrdered,
}

/// Architectural stack growth direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HardwareStackDirection {
    /// The provider cannot characterize stack growth honestly.
    Unknown,
    /// Stacks grow toward lower addresses.
    Down,
    /// Stacks grow toward higher addresses.
    Up,
}

bitflags! {
    /// Native atomic-width support surfaced by a hardware provider.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct HardwareAtomicWidthSet: u32 {
        /// Native 8-bit atomics are supported.
        const WIDTH_8   = 1 << 0;
        /// Native 16-bit atomics are supported.
        const WIDTH_16  = 1 << 1;
        /// Native 32-bit atomics are supported.
        const WIDTH_32  = 1 << 2;
        /// Native 64-bit atomics are supported.
        const WIDTH_64  = 1 << 3;
        /// Native 128-bit atomics are supported.
        const WIDTH_128 = 1 << 4;
    }
}

bitflags! {
    /// Effective SIMD surface available at runtime on the current machine.
    ///
    /// These flags describe SIMD/vector features that are both implemented by the
    /// hardware and usable under the current runtime/OS context-switch model.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct HardwareSimdSet: u64 {
        /// x86 SSE.
        const SSE      = 1 << 0;
        /// x86 SSE2.
        const SSE2     = 1 << 1;
        /// x86 SSE3.
        const SSE3     = 1 << 2;
        /// x86 SSSE3.
        const SSSE3    = 1 << 3;
        /// x86 SSE4.1.
        const SSE4_1   = 1 << 4;
        /// x86 SSE4.2.
        const SSE4_2   = 1 << 5;
        /// x86 AVX.
        const AVX      = 1 << 6;
        /// x86 AVX2.
        const AVX2     = 1 << 7;
        /// x86 AVX-512 Foundation.
        const AVX512F  = 1 << 8;
        /// Arm/AArch64 Advanced SIMD / NEON.
        const NEON     = 1 << 9;
        /// AArch64 Scalable Vector Extension.
        const SVE      = 1 << 10;
        /// AArch64 Scalable Vector Extension 2.
        const SVE2     = 1 << 11;
        /// RISC-V vector extension.
        const RVV      = 1 << 12;
    }
}

/// Stable CPU-facing execution-model description.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HardwareCpuDescription {
    /// Coarse architecture identifier.
    pub architecture: HardwareCpuArchitecture,
    /// Coarse vendor or manufacturer identity.
    pub vendor: HardwareCpuVendor,
    /// Target endianness.
    pub endianness: HardwareEndian,
    /// Data cache line size relevant to padding and false-sharing avoidance, when known.
    pub cache_line_bytes: Option<usize>,
    /// Coarse hardware memory-ordering model.
    pub memory_ordering: HardwareMemoryOrdering,
    /// Pointer width in bits for the selected target.
    pub pointer_width_bits: u16,
    /// Native atomic-width support.
    pub atomic_widths: HardwareAtomicWidthSet,
    /// Runtime-usable SIMD/vector feature surface.
    pub simd: HardwareSimdSet,
}

/// Stack-ABI facts relevant to user-space context setup and green-thread stacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HardwareStackAbi {
    /// Minimum required stack-pointer alignment at entry.
    pub min_stack_alignment: usize,
    /// Architectural red-zone size below the active stack pointer in bytes.
    pub red_zone_bytes: usize,
    /// Stack growth direction.
    pub direction: HardwareStackDirection,
    /// Whether a guard page or equivalent limit mechanism is required, when known.
    pub guard_required: Option<bool>,
}

impl From<crate::pal::context::ContextStackDirection> for HardwareStackDirection {
    fn from(value: crate::pal::context::ContextStackDirection) -> Self {
        match value {
            crate::pal::context::ContextStackDirection::Unknown => Self::Unknown,
            crate::pal::context::ContextStackDirection::Down => Self::Down,
            crate::pal::context::ContextStackDirection::Up => Self::Up,
        }
    }
}

/// Coarse topology summary surfaced without allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HardwareTopologySummary {
    /// Number of logical CPUs, when known.
    pub logical_cpu_count: Option<usize>,
    /// Number of physical or topology-defined cores, when known.
    pub core_count: Option<usize>,
    /// Number of cluster or LLC domains, when known.
    pub cluster_count: Option<usize>,
    /// Number of package or socket nodes, when known.
    pub package_count: Option<usize>,
    /// Number of NUMA nodes, when known.
    pub numa_node_count: Option<usize>,
    /// Number of heterogeneous core classes, when known.
    pub core_class_count: Option<usize>,
}

impl HardwareTopologySummary {
    /// Returns an empty topology summary with no known counts.
    #[must_use]
    pub const fn unknown() -> Self {
        Self {
            logical_cpu_count: None,
            core_count: None,
            cluster_count: None,
            package_count: None,
            numa_node_count: None,
            core_class_count: None,
        }
    }
}

/// Summary of a caller-buffered hardware enumeration write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HardwareWriteSummary {
    /// Total number of items the provider could have written.
    pub total: usize,
    /// Number of items actually written into the supplied buffer.
    pub written: usize,
}

impl HardwareWriteSummary {
    /// Creates a new enumeration-write summary.
    #[must_use]
    pub const fn new(total: usize, written: usize) -> Self {
        Self { total, written }
    }
}
