//! Canonical carrier vocabulary for hardware-bound execution resources.
//!
//! A `courier` is Fusion's authority-scoped execution manager. A `carrier` is the hardware-facing
//! execution resource the courier may be bound onto. This module keeps that distinction honest by
//! collecting the PAL's topology, placement, and observation truth into one substrate-facing
//! language that executors can use without rummaging through three unrelated contracts.

use bitflags::bitflags;
use fusion_pal::contract::pal::{
    HardwareBaseContract as _,
    HardwareError,
    HardwareSupport,
    HardwareTopologyQueryContract as _,
    HardwareTopologySummary,
    HardwareWriteSummary,
};
use fusion_pal::sys::cpu::system_cpu;

use super::{
    ThreadClusterId,
    ThreadConstraintMode,
    ThreadCoreClassId,
    ThreadCoreId,
    ThreadExecutionLocation,
    ThreadGuarantee,
    ThreadHandle,
    ThreadId,
    ThreadLogicalCpuId,
    ThreadMigrationPolicy,
    ThreadObservation,
    ThreadPlacementOutcome,
    ThreadPlacementPhase,
    ThreadPlacementTarget,
    ThreadRunState,
    ThreadSchedulerObservation,
    ThreadSupport,
    system_thread,
};

pub use fusion_pal::contract::pal::HardwareTopologyNodeId;

/// Carrier-facing placement constraint strength.
pub type CarrierConstraintMode = ThreadConstraintMode;
/// Carrier-facing migration policy.
pub type CarrierMigrationPolicy = ThreadMigrationPolicy;
/// Carrier-facing placement phase.
pub type CarrierPlacementPhase = ThreadPlacementPhase;
/// Carrier-facing guarantee ladder.
pub type CarrierGuarantee = ThreadGuarantee;
/// Carrier-facing topology summary.
pub type CarrierTopologySummary = HardwareTopologySummary;

/// Canonical carrier-count policy resolved from observed machine topology.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CarrierCountPolicy {
    /// Caller supplies the carrier count directly.
    Fixed(usize),
    /// One carrier per scheduler-visible logical CPU / hyperthread.
    PerLogicalCpu,
    /// One carrier per physical or topology-defined core.
    PerCore,
    /// One carrier per shared cluster or LLC domain.
    PerCluster,
    /// One carrier per package or socket.
    PerPackage,
    /// One carrier per NUMA node.
    PerNumaNode,
    /// One carrier per heterogeneous core class.
    PerCoreClass,
}

/// Canonical carrier-locality bias.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CarrierLocalityPolicy {
    /// Inherit backend defaults.
    Inherit,
    /// Prefer densely packed carriers before spreading outward.
    Compact,
    /// Prefer spreading carriers across the visible topology domains.
    Spread,
    /// Prefer local affinity first, widening only when required.
    LocalFirst,
}

/// Canonical spawn-locality preference when admitting new courier-local runnable work.
///
/// This policy describes how strongly a scheduler should try to preserve the origin carrier's
/// locality when placing newly spawned work. Exact same-carrier admission remains the strongest
/// truthful preference when the origin is already executing on one live carrier of the target
/// courier. These variants describe the widening boundary after that exact match either fails or
/// is unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CarrierSpawnLocalityPolicy {
    /// Inherit runtime defaults.
    Inherit,
    /// Prefer the same logical CPU first, then widen outward honestly.
    SameLogicalCpu,
    /// Prefer the same core first, then widen outward honestly.
    SameCore,
    /// Prefer the same cluster or LLC domain first, then widen outward honestly.
    SameCluster,
    /// Prefer the same package or socket first, then widen outward honestly.
    SamePackage,
    /// Prefer the same NUMA node first, then widen outward honestly.
    SameNumaNode,
    /// Do not preserve origin locality intentionally.
    Any,
}

/// Canonical stealing boundary between carriers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CarrierStealPolicy {
    /// Disable stealing across carrier queues.
    LocalOnly,
    /// Allow stealing only within the same cluster or LLC domain.
    SameCluster,
    /// Allow stealing only within the same package or socket.
    SamePackage,
    /// Allow stealing only within the same NUMA node.
    SameNumaNode,
    /// Allow stealing across the whole carrier set.
    Global,
}

/// Canonical workload-biased carrier profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CarrierWorkloadProfile {
    /// Small general-purpose work where SMT/hyperthread lanes are often useful.
    GeneralPurpose,
    /// Dedicated or contention-sensitive work where one carrier per core is preferred.
    DedicatedCore,
    /// Locality-sensitive work biased toward package/socket boundaries.
    PackageLocal,
    /// Locality-sensitive work biased toward NUMA-node boundaries.
    NumaLocal,
}

impl CarrierWorkloadProfile {
    /// Returns the default carrier-count policy for this workload profile.
    #[must_use]
    pub const fn count_policy(self) -> CarrierCountPolicy {
        match self {
            Self::GeneralPurpose => CarrierCountPolicy::PerLogicalCpu,
            Self::DedicatedCore => CarrierCountPolicy::PerCore,
            Self::PackageLocal => CarrierCountPolicy::PerPackage,
            Self::NumaLocal => CarrierCountPolicy::PerNumaNode,
        }
    }

    /// Returns the default locality bias for this workload profile.
    #[must_use]
    pub const fn locality_policy(self) -> CarrierLocalityPolicy {
        match self {
            Self::GeneralPurpose => CarrierLocalityPolicy::Spread,
            Self::DedicatedCore => CarrierLocalityPolicy::Compact,
            Self::PackageLocal | Self::NumaLocal => CarrierLocalityPolicy::LocalFirst,
        }
    }

    /// Returns the default spawn-locality preference for this workload profile.
    #[must_use]
    pub const fn spawn_locality_policy(self) -> CarrierSpawnLocalityPolicy {
        match self {
            Self::GeneralPurpose | Self::DedicatedCore => CarrierSpawnLocalityPolicy::SameCore,
            Self::PackageLocal => CarrierSpawnLocalityPolicy::SamePackage,
            Self::NumaLocal => CarrierSpawnLocalityPolicy::SameNumaNode,
        }
    }

    /// Returns the default stealing boundary for this workload profile.
    #[must_use]
    pub const fn steal_policy(self) -> CarrierStealPolicy {
        match self {
            Self::GeneralPurpose => CarrierStealPolicy::Global,
            Self::DedicatedCore => CarrierStealPolicy::SameCluster,
            Self::PackageLocal => CarrierStealPolicy::SamePackage,
            Self::NumaLocal => CarrierStealPolicy::SameNumaNode,
        }
    }
}

/// Returns the relative locality rank between one origin and one candidate carrier location.
///
/// Lower ranks are more local and therefore better. `None` means the supplied policy does not ask
/// for locality preservation or the candidate does not match the origin at any truthful boundary.
#[must_use]
pub fn carrier_spawn_locality_rank(
    policy: CarrierSpawnLocalityPolicy,
    origin: CarrierLocation,
    candidate: CarrierLocation,
) -> Option<u8> {
    let widest_rank = match policy {
        CarrierSpawnLocalityPolicy::Inherit | CarrierSpawnLocalityPolicy::Any => return None,
        CarrierSpawnLocalityPolicy::SameLogicalCpu => 0,
        CarrierSpawnLocalityPolicy::SameCore => 1,
        CarrierSpawnLocalityPolicy::SameCluster => 2,
        CarrierSpawnLocalityPolicy::SamePackage => 3,
        CarrierSpawnLocalityPolicy::SameNumaNode => 4,
    };

    if origin.logical_cpu.is_some() && origin.logical_cpu == candidate.logical_cpu {
        return Some(0);
    }
    if widest_rank >= 1 && origin.core.is_some() && origin.core == candidate.core {
        return Some(1);
    }
    if widest_rank >= 2 && origin.cluster.is_some() && origin.cluster == candidate.cluster {
        return Some(2);
    }
    if widest_rank >= 3 && origin.package.is_some() && origin.package == candidate.package {
        return Some(3);
    }
    if widest_rank >= 4 && origin.numa_node.is_some() && origin.numa_node == candidate.numa_node {
        return Some(4);
    }
    None
}

bitflags! {
    /// Canonical carrier capability surface synthesized from hardware and thread truth.
    ///
    /// These flags do not invent new platform abilities. They summarize what the selected PAL can
    /// honestly observe or request about the machine execution resources a courier may ride on.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct CarrierCaps: u32 {
        /// The current executing carrier can be observed.
        const CURRENT_OBSERVE      = 1 << 0;
        /// Another live thread handle's effective carrier can be observed.
        const HANDLE_OBSERVE       = 1 << 1;
        /// A coarse topology summary can be surfaced.
        const TOPOLOGY_SUMMARY     = 1 << 2;
        /// Logical CPU identifiers can be enumerated.
        const LOGICAL_CPUS         = 1 << 3;
        /// Core identifiers can be enumerated.
        const CORES                = 1 << 4;
        /// Cluster / LLC-domain identifiers can be enumerated.
        const CLUSTERS             = 1 << 5;
        /// Package / socket identifiers can be enumerated.
        const PACKAGES             = 1 << 6;
        /// NUMA-node identifiers can be enumerated.
        const NUMA_NODES           = 1 << 7;
        /// Heterogeneous core-class identifiers can be enumerated.
        const CORE_CLASSES         = 1 << 8;
        /// Logical CPU affinity can be requested.
        const LOGICAL_CPU_AFFINITY = 1 << 9;
        /// Package affinity can be requested.
        const PACKAGE_AFFINITY     = 1 << 10;
        /// NUMA-node affinity can be requested.
        const NUMA_AFFINITY        = 1 << 11;
        /// Core-class affinity can be requested.
        const CORE_CLASS_AFFINITY  = 1 << 12;
        /// Effective placement outcomes can be observed.
        const EFFECTIVE_OBSERVE    = 1 << 13;
        /// Migration history or current migration state can be observed.
        const MIGRATION_OBSERVE    = 1 << 14;
    }
}

/// Canonical carrier-support snapshot for the selected machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CarrierSupport {
    /// Synthesized carrier capability flags.
    pub caps: CarrierCaps,
    /// Underlying hardware support contributing topology truth.
    pub hardware: HardwareSupport,
    /// Underlying thread support contributing observation and placement truth.
    pub thread: ThreadSupport,
}

impl CarrierSupport {
    /// Returns an explicitly unsupported carrier surface.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            caps: CarrierCaps::empty(),
            hardware: HardwareSupport::unsupported(),
            thread: ThreadSupport::unsupported(),
        }
    }
}

/// One exact observed carrier location.
///
/// This is intentionally the executor-facing hardware address of where work is or was running.
/// When only the logical CPU is known, the other fields remain `None` rather than being guessed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CarrierLocation {
    /// Scheduler-visible hardware thread / hyperthread.
    pub logical_cpu: Option<ThreadLogicalCpuId>,
    /// Physical or topology-defined core.
    pub core: Option<ThreadCoreId>,
    /// Cluster or LLC domain.
    pub cluster: Option<ThreadClusterId>,
    /// Package or socket topology node.
    pub package: Option<HardwareTopologyNodeId>,
    /// NUMA topology node.
    pub numa_node: Option<HardwareTopologyNodeId>,
    /// Heterogeneous core class.
    pub core_class: Option<ThreadCoreClassId>,
}

impl CarrierLocation {
    /// Returns an empty location with no observable hardware placement facts.
    #[must_use]
    pub const fn unknown() -> Self {
        Self {
            logical_cpu: None,
            core: None,
            cluster: None,
            package: None,
            numa_node: None,
            core_class: None,
        }
    }

    /// Returns the most concrete single execution slot the machine can currently name.
    ///
    /// Logical CPUs represent the most specific scheduler-visible execution slot. When no logical
    /// CPU identity is available, the next-most-specific observed coordinate is returned.
    #[must_use]
    pub const fn concrete_slot(self) -> Option<CarrierConcreteSlot> {
        if let Some(logical_cpu) = self.logical_cpu {
            return Some(CarrierConcreteSlot::LogicalCpu(logical_cpu));
        }
        if let Some(core) = self.core {
            return Some(CarrierConcreteSlot::Core(core));
        }
        if let Some(cluster) = self.cluster {
            return Some(CarrierConcreteSlot::Cluster(cluster));
        }
        if let Some(package) = self.package {
            return Some(CarrierConcreteSlot::Package(package));
        }
        if let Some(numa_node) = self.numa_node {
            return Some(CarrierConcreteSlot::NumaNode(numa_node));
        }
        if let Some(core_class) = self.core_class {
            return Some(CarrierConcreteSlot::CoreClass(core_class));
        }
        None
    }
}

impl From<ThreadExecutionLocation> for CarrierLocation {
    fn from(value: ThreadExecutionLocation) -> Self {
        Self {
            logical_cpu: value.logical_cpu,
            core: value.core,
            cluster: value.cluster,
            package: value.package,
            numa_node: value.numa_node,
            core_class: value.core_class,
        }
    }
}

impl From<CarrierLocation> for ThreadExecutionLocation {
    fn from(value: CarrierLocation) -> Self {
        Self {
            logical_cpu: value.logical_cpu,
            core: value.core,
            cluster: value.cluster,
            package: value.package,
            numa_node: value.numa_node,
            core_class: value.core_class,
        }
    }
}

/// One exact hardware slot or aggregate placement domain surfaced by carrier observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CarrierConcreteSlot {
    LogicalCpu(ThreadLogicalCpuId),
    Core(ThreadCoreId),
    Cluster(ThreadClusterId),
    Package(HardwareTopologyNodeId),
    NumaNode(HardwareTopologyNodeId),
    CoreClass(ThreadCoreClassId),
}

/// Canonical placement outcome for one carrier-bound execution context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CarrierPlacementOutcome {
    /// Effective guarantee strength.
    pub guarantee: CarrierGuarantee,
    /// Phase at which placement was or will be applied.
    pub phase: CarrierPlacementPhase,
    /// Effective observed carrier location.
    pub location: CarrierLocation,
}

impl From<ThreadPlacementOutcome> for CarrierPlacementOutcome {
    fn from(value: ThreadPlacementOutcome) -> Self {
        Self {
            guarantee: value.guarantee,
            phase: value.phase,
            location: value.location.into(),
        }
    }
}

/// Canonical observation snapshot for one thread-bound execution carrier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CarrierObservation {
    /// Observed thread identity owning the execution context.
    pub thread_id: ThreadId,
    /// Coarse run-state classification for the owning execution context.
    pub run_state: ThreadRunState,
    /// Effective scheduler observation for that execution context.
    pub scheduler: ThreadSchedulerObservation,
    /// Direct observed hardware location.
    pub location: CarrierLocation,
    /// Effective placement outcome as the backend can justify it.
    pub placement: CarrierPlacementOutcome,
}

impl From<ThreadObservation> for CarrierObservation {
    fn from(value: ThreadObservation) -> Self {
        Self {
            thread_id: value.id,
            run_state: value.run_state,
            scheduler: value.scheduler,
            location: value.location.into(),
            placement: value.placement.into(),
        }
    }
}

/// Canonical carrier selection targets used by executor-side placement policy.
///
/// This is intentionally richer than the current PAL thread-placement request surface. Some
/// targets, such as explicit `Core` or `Cluster` requests, may be representable canonically here
/// before PAL can honestly honor them. Callers must convert explicitly when crossing into PAL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CarrierTarget<'a> {
    LogicalCpus(&'a [ThreadLogicalCpuId]),
    Cores(&'a [ThreadCoreId]),
    Clusters(&'a [ThreadClusterId]),
    Packages(&'a [HardwareTopologyNodeId]),
    NumaNodes(&'a [HardwareTopologyNodeId]),
    CoreClasses(&'a [ThreadCoreClassId]),
}

/// Executor-facing carrier placement request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CarrierPlacementRequest<'a> {
    /// Requested carrier target domains and identifiers.
    pub targets: &'a [CarrierTarget<'a>],
    /// Strength of the placement request.
    pub mode: CarrierConstraintMode,
    /// When the placement should be applied.
    pub phase: CarrierPlacementPhase,
    /// Requested migration policy after startup.
    pub migration: CarrierMigrationPolicy,
}

impl CarrierPlacementRequest<'_> {
    /// Returns an empty request that inherits platform defaults.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            targets: &[],
            mode: CarrierConstraintMode::Prefer,
            phase: CarrierPlacementPhase::Inherit,
            migration: CarrierMigrationPolicy::Inherit,
        }
    }

    /// Returns `true` when the request contains no explicit placement targets.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }

    /// Returns `true` when every target is currently representable by the PAL thread-placement
    /// contract.
    #[must_use]
    pub fn is_pal_thread_placeable(&self) -> bool {
        self.targets.iter().all(|target| {
            matches!(
                target,
                CarrierTarget::LogicalCpus(_)
                    | CarrierTarget::Packages(_)
                    | CarrierTarget::NumaNodes(_)
                    | CarrierTarget::CoreClasses(_)
            )
        })
    }

    /// Converts this canonical carrier request into the narrower PAL thread-placement request.
    ///
    /// `output` is caller-owned scratch space so this stays no-alloc and honest on bare metal.
    ///
    /// # Errors
    ///
    /// Returns an error when the request uses canonical targets that PAL thread placement cannot
    /// currently express, or when `output` is too small for the number of converted targets.
    pub fn write_thread_request<'a>(
        &'a self,
        output: &'a mut [ThreadPlacementTarget<'a>],
    ) -> Result<super::ThreadPlacementRequest<'a>, CarrierPlacementConversionError> {
        if output.len() < self.targets.len() {
            return Err(CarrierPlacementConversionError::OutputTooSmall);
        }

        let mut written = 0usize;
        for target in self.targets.iter().copied() {
            output[written] = match target {
                CarrierTarget::LogicalCpus(cpus) => ThreadPlacementTarget::LogicalCpus(cpus),
                CarrierTarget::Packages(packages) => ThreadPlacementTarget::Packages(packages),
                CarrierTarget::NumaNodes(nodes) => ThreadPlacementTarget::NumaNodes(nodes),
                CarrierTarget::CoreClasses(classes) => ThreadPlacementTarget::CoreClasses(classes),
                CarrierTarget::Cores(_) | CarrierTarget::Clusters(_) => {
                    return Err(CarrierPlacementConversionError::UnsupportedTarget(
                        target.kind(),
                    ));
                }
            };
            written = written.saturating_add(1);
        }

        Ok(super::ThreadPlacementRequest {
            targets: &output[..written],
            mode: self.mode,
            phase: self.phase,
            migration: self.migration,
        })
    }
}

impl Default for CarrierPlacementRequest<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl CarrierTarget<'_> {
    /// Returns the target-domain kind represented by this selector.
    #[must_use]
    pub const fn kind(self) -> CarrierTargetKind {
        match self {
            Self::LogicalCpus(_) => CarrierTargetKind::LogicalCpu,
            Self::Cores(_) => CarrierTargetKind::Core,
            Self::Clusters(_) => CarrierTargetKind::Cluster,
            Self::Packages(_) => CarrierTargetKind::Package,
            Self::NumaNodes(_) => CarrierTargetKind::NumaNode,
            Self::CoreClasses(_) => CarrierTargetKind::CoreClass,
        }
    }
}

/// Coarse target-domain classifier for canonical carrier placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CarrierTargetKind {
    LogicalCpu,
    Core,
    Cluster,
    Package,
    NumaNode,
    CoreClass,
}

/// Conversion failure while lowering one canonical carrier request into PAL thread placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CarrierPlacementConversionError {
    /// The caller supplied too little scratch space for the converted target list.
    OutputTooSmall,
    /// The canonical target kind is not expressible by the current PAL thread-placement contract.
    UnsupportedTarget(CarrierTargetKind),
}

/// Resolves one carrier-count policy from a coarse topology summary.
#[must_use]
pub const fn carrier_count_from_summary(
    summary: CarrierTopologySummary,
    policy: CarrierCountPolicy,
) -> Option<usize> {
    match policy {
        CarrierCountPolicy::Fixed(count) => {
            if count == 0 {
                None
            } else {
                Some(count)
            }
        }
        CarrierCountPolicy::PerLogicalCpu => summary.logical_cpu_count,
        CarrierCountPolicy::PerCore => {
            if let Some(count) = summary.core_count {
                Some(count)
            } else {
                summary.logical_cpu_count
            }
        }
        CarrierCountPolicy::PerCluster => summary.cluster_count,
        CarrierCountPolicy::PerPackage => summary.package_count,
        CarrierCountPolicy::PerNumaNode => summary.numa_node_count,
        CarrierCountPolicy::PerCoreClass => summary.core_class_count,
    }
}

/// Resolves one workload profile into a carrier count using the visible topology summary.
#[must_use]
pub const fn carrier_count_for_profile(
    summary: CarrierTopologySummary,
    profile: CarrierWorkloadProfile,
) -> Option<usize> {
    carrier_count_from_summary(summary, profile.count_policy())
}

/// Canonical carrier authority over the selected machine.
///
/// PAL still owns the hardware truth. This wrapper exists so executor and courier policy can talk
/// about carriers in one place without scattering that truth across separate hardware and thread
/// APIs.
#[derive(Debug, Clone, Copy, Default)]
pub struct CarrierSystem;

/// Returns the canonical carrier authority for the current machine.
#[must_use]
pub const fn system_carrier() -> CarrierSystem {
    CarrierSystem
}

impl CarrierSystem {
    /// Returns the synthesized carrier support surface for the current machine.
    #[must_use]
    pub fn support(&self) -> CarrierSupport {
        let hardware = system_cpu().support();
        let thread = system_thread().support();
        let mut caps = CarrierCaps::empty();

        if thread
            .lifecycle
            .caps
            .contains(super::ThreadLifecycleCaps::CURRENT_OBSERVE)
        {
            caps |= CarrierCaps::CURRENT_OBSERVE;
        }
        if thread
            .lifecycle
            .caps
            .contains(super::ThreadLifecycleCaps::HANDLE_OBSERVE)
        {
            caps |= CarrierCaps::HANDLE_OBSERVE;
        }
        if hardware
            .topology
            .caps
            .contains(fusion_pal::contract::pal::HardwareTopologyCaps::SUMMARY)
        {
            caps |= CarrierCaps::TOPOLOGY_SUMMARY;
        }
        if hardware
            .topology
            .caps
            .contains(fusion_pal::contract::pal::HardwareTopologyCaps::LOGICAL_CPUS)
        {
            caps |= CarrierCaps::LOGICAL_CPUS;
        }
        if hardware
            .topology
            .caps
            .contains(fusion_pal::contract::pal::HardwareTopologyCaps::CORES)
        {
            caps |= CarrierCaps::CORES;
        }
        if hardware
            .topology
            .caps
            .contains(fusion_pal::contract::pal::HardwareTopologyCaps::CLUSTERS)
        {
            caps |= CarrierCaps::CLUSTERS;
        }
        if hardware
            .topology
            .caps
            .contains(fusion_pal::contract::pal::HardwareTopologyCaps::PACKAGES)
        {
            caps |= CarrierCaps::PACKAGES;
        }
        if hardware
            .topology
            .caps
            .contains(fusion_pal::contract::pal::HardwareTopologyCaps::NUMA_NODES)
        {
            caps |= CarrierCaps::NUMA_NODES;
        }
        if hardware
            .topology
            .caps
            .contains(fusion_pal::contract::pal::HardwareTopologyCaps::CORE_CLASSES)
        {
            caps |= CarrierCaps::CORE_CLASSES;
        }
        if thread
            .placement
            .caps
            .contains(super::ThreadPlacementCaps::LOGICAL_CPU_AFFINITY)
        {
            caps |= CarrierCaps::LOGICAL_CPU_AFFINITY;
        }
        if thread
            .placement
            .caps
            .contains(super::ThreadPlacementCaps::PACKAGE_AFFINITY)
        {
            caps |= CarrierCaps::PACKAGE_AFFINITY;
        }
        if thread
            .placement
            .caps
            .contains(super::ThreadPlacementCaps::NUMA_AFFINITY)
        {
            caps |= CarrierCaps::NUMA_AFFINITY;
        }
        if thread
            .placement
            .caps
            .contains(super::ThreadPlacementCaps::CORE_CLASS_AFFINITY)
        {
            caps |= CarrierCaps::CORE_CLASS_AFFINITY;
        }
        if thread
            .placement
            .caps
            .contains(super::ThreadPlacementCaps::EFFECTIVE_OBSERVE)
        {
            caps |= CarrierCaps::EFFECTIVE_OBSERVE;
        }
        if thread
            .placement
            .caps
            .contains(super::ThreadPlacementCaps::MIGRATION_OBSERVE)
        {
            caps |= CarrierCaps::MIGRATION_OBSERVE;
        }

        CarrierSupport {
            caps,
            hardware,
            thread,
        }
    }

    /// Returns a coarse carrier-topology summary.
    ///
    /// # Errors
    ///
    /// Returns any honest topology-summary failure surfaced by PAL.
    pub fn topology_summary(&self) -> Result<CarrierTopologySummary, HardwareError> {
        system_cpu().topology_summary()
    }

    /// Resolves one canonical carrier-count policy against the visible machine topology.
    ///
    /// # Errors
    ///
    /// Returns any honest topology-summary failure surfaced by PAL.
    pub fn resolve_count(
        &self,
        policy: CarrierCountPolicy,
    ) -> Result<Option<usize>, HardwareError> {
        self.topology_summary()
            .map(|summary| carrier_count_from_summary(summary, policy))
    }

    /// Resolves one workload profile into a concrete carrier count using visible topology.
    ///
    /// # Errors
    ///
    /// Returns any honest topology-summary failure surfaced by PAL.
    pub fn resolve_profile_count(
        &self,
        profile: CarrierWorkloadProfile,
    ) -> Result<Option<usize>, HardwareError> {
        self.topology_summary()
            .map(|summary| carrier_count_for_profile(summary, profile))
    }

    /// Writes scheduler-visible logical carrier identifiers into `output`.
    pub fn write_logical_cpus(
        &self,
        output: &mut [ThreadLogicalCpuId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        system_cpu().write_logical_cpus(output)
    }

    /// Writes core identifiers into `output`.
    pub fn write_cores(
        &self,
        output: &mut [ThreadCoreId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        system_cpu().write_cores(output)
    }

    /// Writes cluster identifiers into `output`.
    pub fn write_clusters(
        &self,
        output: &mut [ThreadClusterId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        system_cpu().write_clusters(output)
    }

    /// Writes package identifiers into `output`.
    pub fn write_packages(
        &self,
        output: &mut [HardwareTopologyNodeId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        system_cpu().write_packages(output)
    }

    /// Writes NUMA-node identifiers into `output`.
    pub fn write_numa_nodes(
        &self,
        output: &mut [HardwareTopologyNodeId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        system_cpu().write_numa_nodes(output)
    }

    /// Writes heterogeneous core-class identifiers into `output`.
    pub fn write_core_classes(
        &self,
        output: &mut [ThreadCoreClassId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        system_cpu().write_core_classes(output)
    }

    /// Observes the carrier currently executing this thread.
    ///
    /// # Errors
    ///
    /// Returns any honest thread-observation failure.
    pub fn observe_current(&self) -> Result<CarrierObservation, super::ThreadError> {
        system_thread().observe_current().map(Into::into)
    }

    /// Observes the effective carrier of another live thread handle.
    ///
    /// # Errors
    ///
    /// Returns any honest thread-observation failure.
    pub fn observe(&self, handle: &ThreadHandle) -> Result<CarrierObservation, super::ThreadError> {
        system_thread().observe(handle).map(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carrier_count_policies_resolve_expected_topology_domain() {
        let summary = CarrierTopologySummary {
            logical_cpu_count: Some(16),
            core_count: Some(8),
            cluster_count: Some(4),
            package_count: Some(2),
            numa_node_count: Some(2),
            core_class_count: Some(3),
        };
        assert_eq!(
            carrier_count_from_summary(summary, CarrierCountPolicy::PerLogicalCpu),
            Some(16)
        );
        assert_eq!(
            carrier_count_from_summary(summary, CarrierCountPolicy::PerCore),
            Some(8)
        );
        assert_eq!(
            carrier_count_from_summary(summary, CarrierCountPolicy::PerCluster),
            Some(4)
        );
        assert_eq!(
            carrier_count_from_summary(summary, CarrierCountPolicy::PerPackage),
            Some(2)
        );
        assert_eq!(
            carrier_count_from_summary(summary, CarrierCountPolicy::PerNumaNode),
            Some(2)
        );
        assert_eq!(
            carrier_count_from_summary(summary, CarrierCountPolicy::PerCoreClass),
            Some(3)
        );
    }

    #[test]
    fn carrier_workload_profiles_choose_expected_defaults() {
        assert_eq!(
            CarrierWorkloadProfile::GeneralPurpose.count_policy(),
            CarrierCountPolicy::PerLogicalCpu
        );
        assert_eq!(
            CarrierWorkloadProfile::GeneralPurpose.spawn_locality_policy(),
            CarrierSpawnLocalityPolicy::SameCore
        );
        assert_eq!(
            CarrierWorkloadProfile::DedicatedCore.count_policy(),
            CarrierCountPolicy::PerCore
        );
        assert_eq!(
            CarrierWorkloadProfile::DedicatedCore.spawn_locality_policy(),
            CarrierSpawnLocalityPolicy::SameCore
        );
        assert_eq!(
            CarrierWorkloadProfile::PackageLocal.steal_policy(),
            CarrierStealPolicy::SamePackage
        );
        assert_eq!(
            CarrierWorkloadProfile::PackageLocal.spawn_locality_policy(),
            CarrierSpawnLocalityPolicy::SamePackage
        );
        assert_eq!(
            CarrierWorkloadProfile::NumaLocal.locality_policy(),
            CarrierLocalityPolicy::LocalFirst
        );
        assert_eq!(
            CarrierWorkloadProfile::NumaLocal.spawn_locality_policy(),
            CarrierSpawnLocalityPolicy::SameNumaNode
        );
    }

    #[test]
    fn carrier_request_reports_pal_placeability_honestly() {
        let logical = [ThreadLogicalCpuId {
            group: super::super::ThreadProcessorGroupId(0),
            index: 0,
        }];
        let cores = [ThreadCoreId(0)];
        let request = CarrierPlacementRequest {
            targets: &[
                CarrierTarget::LogicalCpus(&logical),
                CarrierTarget::Cores(&cores),
            ],
            ..CarrierPlacementRequest::new()
        };
        assert!(!request.is_pal_thread_placeable());
    }

    #[test]
    fn carrier_request_converts_placeable_targets() {
        let logical = [ThreadLogicalCpuId {
            group: super::super::ThreadProcessorGroupId(0),
            index: 0,
        }];
        let core_classes = [ThreadCoreClassId(7)];
        let request = CarrierPlacementRequest {
            targets: &[
                CarrierTarget::LogicalCpus(&logical),
                CarrierTarget::CoreClasses(&core_classes),
            ],
            ..CarrierPlacementRequest::new()
        };
        let mut output = [ThreadPlacementTarget::LogicalCpus(&[]); 2];
        let converted = request
            .write_thread_request(&mut output)
            .expect("thread request conversion should succeed");
        assert_eq!(converted.targets.len(), 2);
        assert!(matches!(
            converted.targets[0],
            ThreadPlacementTarget::LogicalCpus(_)
        ));
        assert!(matches!(
            converted.targets[1],
            ThreadPlacementTarget::CoreClasses(_)
        ));
    }
}
