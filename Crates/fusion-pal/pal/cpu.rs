//! Selected CPU-facing hardware composition surface.
//!
//! This module sits inside [`crate::pal`]: it consumes the selected PAL platform backend, fills in
//! compile-time ISA and ABI truth where that is the only honest source today, and exposes the
//! canonical CPU-facing provider through the backend-neutral HAL contract.

use crate::contract::hal::{
    HardwareAtomicWidthSet,
    HardwareAuthoritySet,
    HardwareBase,
    HardwareCpuArchitecture,
    HardwareCpuCaps,
    HardwareCpuDescription,
    HardwareCpuQuery,
    HardwareCpuSupport,
    HardwareCpuVendor,
    HardwareEndian,
    HardwareError,
    HardwareGuarantee,
    HardwareImplementationKind,
    HardwareMemoryOrdering,
    HardwareSimdSet,
    HardwareStackAbi,
    HardwareStackDirection,
    HardwareSupport,
    HardwareTopologyNodeId,
    HardwareTopologyQuery,
    HardwareTopologySummary,
    HardwareWriteSummary,
};
use crate::contract::runtime::context::{ContextBase as _, ContextImplementationKind};
use crate::contract::runtime::thread::{
    ThreadClusterId,
    ThreadCoreClassId,
    ThreadCoreId,
    ThreadLogicalCpuId,
};

#[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
/// Cortex-M core-specific CPU helpers.
pub use crate::pal::selected::platform::hal::core;
#[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
/// Cortex-M SoC-specific CPU helpers.
pub use crate::pal::selected::platform::hal::soc;

/// Selected CPU-query provider for the current target.
#[derive(Debug, Clone, Copy, Default)]
pub struct CpuSystem;

impl CpuSystem {
    /// Creates a new selected CPU-query provider.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

/// Returns the selected CPU-query provider for the current target.
#[must_use]
pub const fn system_cpu() -> CpuSystem {
    CpuSystem::new()
}

impl HardwareBase for CpuSystem {
    fn support(&self) -> HardwareSupport {
        let platform_support = crate::pal::selected::platform::hal::system_hardware().support();
        let cache_line_bytes =
            if platform_support.cpu.cache_line_bytes == HardwareGuarantee::Unsupported {
                cache_line_size_guarantee()
            } else {
                platform_support.cpu.cache_line_bytes
            };

        let mut caps = HardwareCpuCaps::DESCRIPTOR
            | HardwareCpuCaps::MEMORY_ORDERING
            | HardwareCpuCaps::ATOMIC_WIDTHS
            | HardwareCpuCaps::STACK_ABI;

        if platform_support.cpu.vendor != HardwareGuarantee::Unsupported {
            caps |= HardwareCpuCaps::VENDOR;
        }
        if cache_line_bytes != HardwareGuarantee::Unsupported {
            caps |= HardwareCpuCaps::CACHE_LINE_BYTES;
        }
        if platform_support.cpu.simd != HardwareGuarantee::Unsupported {
            caps |= HardwareCpuCaps::SIMD;
        }

        HardwareSupport {
            cpu: HardwareCpuSupport {
                caps,
                descriptor: HardwareGuarantee::Verified,
                vendor: platform_support.cpu.vendor,
                cache_line_bytes,
                memory_ordering: HardwareGuarantee::Verified,
                atomic_widths: HardwareGuarantee::Verified,
                stack_abi: stack_abi_guarantee(),
                simd: platform_support.cpu.simd,
                authorities: platform_support.cpu.authorities | HardwareAuthoritySet::ISA,
                implementation: normalized_cpu_implementation(platform_support.cpu.implementation),
            },
            topology: platform_support.topology,
        }
    }
}

impl HardwareCpuQuery for CpuSystem {
    fn cpu_description(&self) -> Result<HardwareCpuDescription, HardwareError> {
        let platform_cpu = crate::pal::selected::platform::hal::system_hardware()
            .cpu_description()
            .ok();

        Ok(HardwareCpuDescription {
            architecture: selected_architecture(),
            vendor: platform_cpu.map_or(HardwareCpuVendor::Unknown, |cpu| cpu.vendor),
            endianness: selected_endianness(),
            cache_line_bytes: platform_cpu
                .and_then(|cpu| cpu.cache_line_bytes)
                .or(selected_cache_line_bytes()),
            memory_ordering: selected_memory_ordering(),
            pointer_width_bits: selected_pointer_width_bits(),
            atomic_widths: selected_atomic_widths(),
            simd: platform_cpu.map_or(HardwareSimdSet::empty(), |cpu| cpu.simd),
        })
    }

    fn stack_abi(&self) -> Result<HardwareStackAbi, HardwareError> {
        let context_support = crate::pal::selected::platform::context::system_context().support();
        if context_support.implementation != ContextImplementationKind::Unsupported {
            return Ok(HardwareStackAbi {
                min_stack_alignment: context_support.min_stack_alignment,
                red_zone_bytes: context_support.red_zone_bytes,
                direction: context_support.stack_direction.into(),
                guard_required: Some(context_support.guard_required),
            });
        }

        Ok(fallback_stack_abi())
    }
}

impl HardwareTopologyQuery for CpuSystem {
    fn topology_summary(&self) -> Result<HardwareTopologySummary, HardwareError> {
        crate::pal::selected::platform::hal::system_hardware().topology_summary()
    }

    fn write_logical_cpus(
        &self,
        output: &mut [ThreadLogicalCpuId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        crate::pal::selected::platform::hal::system_hardware().write_logical_cpus(output)
    }

    fn write_cores(
        &self,
        output: &mut [ThreadCoreId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        crate::pal::selected::platform::hal::system_hardware().write_cores(output)
    }

    fn write_clusters(
        &self,
        output: &mut [ThreadClusterId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        crate::pal::selected::platform::hal::system_hardware().write_clusters(output)
    }

    fn write_packages(
        &self,
        output: &mut [HardwareTopologyNodeId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        crate::pal::selected::platform::hal::system_hardware().write_packages(output)
    }

    fn write_numa_nodes(
        &self,
        output: &mut [HardwareTopologyNodeId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        crate::pal::selected::platform::hal::system_hardware().write_numa_nodes(output)
    }

    fn write_core_classes(
        &self,
        output: &mut [ThreadCoreClassId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        crate::pal::selected::platform::hal::system_hardware().write_core_classes(output)
    }
}

pub(crate) fn stack_abi_guarantee() -> HardwareGuarantee {
    let context_support = crate::pal::selected::platform::context::system_context().support();
    if context_support.implementation != ContextImplementationKind::Unsupported {
        return context_support.guarantee;
    }

    HardwareGuarantee::Verified
}

const fn cache_line_size_guarantee() -> HardwareGuarantee {
    if selected_cache_line_bytes().is_some() {
        HardwareGuarantee::Verified
    } else {
        HardwareGuarantee::Unsupported
    }
}

pub(crate) fn selected_atomic_widths() -> HardwareAtomicWidthSet {
    let mut widths = HardwareAtomicWidthSet::empty();

    #[cfg(target_has_atomic = "8")]
    {
        widths |= HardwareAtomicWidthSet::WIDTH_8;
    }

    #[cfg(target_has_atomic = "16")]
    {
        widths |= HardwareAtomicWidthSet::WIDTH_16;
    }

    #[cfg(target_has_atomic = "32")]
    {
        widths |= HardwareAtomicWidthSet::WIDTH_32;
    }

    #[cfg(target_has_atomic = "64")]
    {
        widths |= HardwareAtomicWidthSet::WIDTH_64;
    }

    #[cfg(target_has_atomic = "128")]
    {
        widths |= HardwareAtomicWidthSet::WIDTH_128;
    }

    widths
}

pub(crate) const fn selected_architecture() -> HardwareCpuArchitecture {
    #[cfg(target_arch = "x86_64")]
    {
        return HardwareCpuArchitecture::X86_64;
    }

    #[cfg(target_arch = "aarch64")]
    {
        return HardwareCpuArchitecture::Aarch64;
    }

    #[cfg(target_arch = "arm")]
    {
        return HardwareCpuArchitecture::Arm;
    }

    #[cfg(target_arch = "riscv64")]
    {
        return HardwareCpuArchitecture::RiscV64;
    }

    #[allow(unreachable_code)]
    HardwareCpuArchitecture::Other
}

pub(crate) const fn selected_endianness() -> HardwareEndian {
    #[cfg(target_endian = "little")]
    {
        return HardwareEndian::Little;
    }

    #[cfg(target_endian = "big")]
    {
        return HardwareEndian::Big;
    }

    #[allow(unreachable_code)]
    HardwareEndian::Unknown
}

pub(crate) const fn selected_memory_ordering() -> HardwareMemoryOrdering {
    #[cfg(target_arch = "x86_64")]
    {
        return HardwareMemoryOrdering::TotalStoreOrder;
    }

    #[cfg(any(target_arch = "aarch64", target_arch = "arm", target_arch = "riscv64"))]
    {
        return HardwareMemoryOrdering::WeaklyOrdered;
    }

    #[allow(unreachable_code)]
    HardwareMemoryOrdering::Unknown
}

pub(crate) const fn selected_cache_line_bytes() -> Option<usize> {
    #[cfg(target_arch = "x86_64")]
    {
        return Some(64);
    }

    #[allow(unreachable_code)]
    None
}

pub(crate) const fn selected_pointer_width_bits() -> u16 {
    #[cfg(target_pointer_width = "64")]
    {
        return 64;
    }

    #[cfg(target_pointer_width = "32")]
    {
        return 32;
    }

    #[cfg(target_pointer_width = "16")]
    {
        return 16;
    }

    #[allow(unreachable_code)]
    0
}

const fn fallback_stack_abi() -> HardwareStackAbi {
    #[cfg(target_arch = "x86_64")]
    {
        return HardwareStackAbi {
            min_stack_alignment: 16,
            red_zone_bytes: 128,
            direction: HardwareStackDirection::Down,
            guard_required: None,
        };
    }

    #[cfg(any(target_arch = "aarch64", target_arch = "arm", target_arch = "riscv64"))]
    {
        return HardwareStackAbi {
            min_stack_alignment: 16,
            red_zone_bytes: 0,
            direction: HardwareStackDirection::Down,
            guard_required: None,
        };
    }

    #[allow(unreachable_code)]
    HardwareStackAbi {
        min_stack_alignment: 1,
        red_zone_bytes: 0,
        direction: HardwareStackDirection::Unknown,
        guard_required: None,
    }
}

const fn normalized_cpu_implementation(
    implementation: HardwareImplementationKind,
) -> HardwareImplementationKind {
    match implementation {
        HardwareImplementationKind::Unsupported => HardwareImplementationKind::Native,
        other => other,
    }
}

#[cfg(all(test, feature = "std", not(target_os = "none")))]
mod tests {
    extern crate std;

    use super::*;
    use crate::contract::hal::{HardwareErrorKind, HardwareTopologyCaps};
    #[cfg(target_os = "linux")]
    use rustix::thread::{self as rustix_thread, CpuSet};

    #[test]
    fn system_cpu_reports_cpu_support() {
        let cpu = system_cpu();
        let support = cpu.support();

        assert!(
            support.cpu.caps.contains(HardwareCpuCaps::DESCRIPTOR),
            "selected HAL should surface a CPU descriptor"
        );
        assert_eq!(
            support.cpu.implementation,
            HardwareImplementationKind::Native
        );
    }

    #[test]
    fn system_cpu_reports_architecture_and_stack_abi() {
        let system = system_cpu();
        let cpu = system.cpu_description().expect("cpu description");
        let stack = system.stack_abi().expect("stack abi");

        #[cfg(target_arch = "x86_64")]
        {
            assert_eq!(cpu.architecture, HardwareCpuArchitecture::X86_64);
            assert_eq!(cpu.cache_line_bytes, Some(64));
            assert_eq!(stack.red_zone_bytes, 128);
            assert!(cpu.simd.contains(HardwareSimdSet::SSE));
            assert!(cpu.simd.contains(HardwareSimdSet::SSE2));
        }

        #[cfg(target_arch = "aarch64")]
        {
            assert_eq!(cpu.architecture, HardwareCpuArchitecture::Aarch64);
            assert_eq!(cpu.cache_line_bytes, None);
            assert_eq!(stack.red_zone_bytes, 0);
        }

        assert_eq!(stack.direction, HardwareStackDirection::Down);
        assert!(stack.min_stack_alignment >= 16);
    }

    #[test]
    #[cfg(not(target_os = "linux"))]
    fn system_cpu_topology_is_honestly_unsupported_for_now() {
        let cpu = system_cpu();

        assert_eq!(
            cpu.support().topology.implementation,
            HardwareImplementationKind::Unsupported
        );
        assert_eq!(
            cpu.topology_summary()
                .expect_err("topology should be unsupported")
                .kind(),
            HardwareErrorKind::Unsupported
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn system_cpu_reports_linux_visible_cpu_topology() {
        let cpu = system_cpu();
        let support = cpu.support();
        let summary = cpu.topology_summary().expect("linux topology summary");
        let cpuset = rustix_thread::sched_getaffinity(None).expect("affinity query should work");

        assert_eq!(
            support.topology.implementation,
            HardwareImplementationKind::Native
        );
        assert!(
            support
                .topology
                .caps
                .contains(HardwareTopologyCaps::LOGICAL_CPUS),
            "linux HAL should enumerate scheduler-visible logical CPUs"
        );
        assert_eq!(summary.logical_cpu_count, Some(cpuset.count() as usize));
        if let Some(core_count) = summary.core_count {
            assert!(
                support.topology.caps.contains(HardwareTopologyCaps::CORES),
                "linux HAL should surface visible physical cores when topology sysfs is available"
            );
            assert!(core_count > 0);
            assert!(core_count <= cpuset.count() as usize);
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn system_cpu_writes_linux_visible_cpu_ids() {
        let cpu = system_cpu();
        let cpuset = rustix_thread::sched_getaffinity(None).expect("affinity query should work");
        let mut output = [ThreadLogicalCpuId {
            group: crate::contract::runtime::thread::ThreadProcessorGroupId(0),
            index: 0,
        }; CpuSet::MAX_CPU];
        let summary = cpu
            .write_logical_cpus(&mut output)
            .expect("linux logical cpu enumeration");

        assert_eq!(summary.total, cpuset.count() as usize);
        assert_eq!(summary.written, summary.total);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn system_cpu_writes_visible_core_ids_when_available() {
        let cpu = system_cpu();
        let summary = cpu.topology_summary().expect("linux topology summary");
        let Some(core_count) = summary.core_count else {
            return;
        };

        let mut output = [crate::contract::runtime::thread::ThreadCoreId(0); CpuSet::MAX_CPU];
        let written = cpu
            .write_cores(&mut output)
            .expect("linux core enumeration");

        assert_eq!(written.total, core_count);
        assert_eq!(written.written, written.total);
    }

    #[test]
    #[cfg(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64")))]
    fn system_cpu_runtime_simd_matches_std_x86_detection() {
        let simd = system_cpu()
            .cpu_description()
            .expect("cpu description")
            .simd;

        assert_eq!(
            simd.contains(HardwareSimdSet::SSE),
            std::arch::is_x86_feature_detected!("sse")
        );
        assert_eq!(
            simd.contains(HardwareSimdSet::SSE2),
            std::arch::is_x86_feature_detected!("sse2")
        );
        assert_eq!(
            simd.contains(HardwareSimdSet::SSE3),
            std::arch::is_x86_feature_detected!("sse3")
        );
        assert_eq!(
            simd.contains(HardwareSimdSet::SSSE3),
            std::arch::is_x86_feature_detected!("ssse3")
        );
        assert_eq!(
            simd.contains(HardwareSimdSet::SSE4_1),
            std::arch::is_x86_feature_detected!("sse4.1")
        );
        assert_eq!(
            simd.contains(HardwareSimdSet::SSE4_2),
            std::arch::is_x86_feature_detected!("sse4.2")
        );
        assert_eq!(
            simd.contains(HardwareSimdSet::AVX),
            std::arch::is_x86_feature_detected!("avx")
        );
        assert_eq!(
            simd.contains(HardwareSimdSet::AVX2),
            std::arch::is_x86_feature_detected!("avx2")
        );
        assert_eq!(
            simd.contains(HardwareSimdSet::AVX512F),
            std::arch::is_x86_feature_detected!("avx512f")
        );
    }
}
