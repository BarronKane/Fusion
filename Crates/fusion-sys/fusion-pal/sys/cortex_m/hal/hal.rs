//! Cortex-M hardware backend.

pub mod core;
#[path = "soc/soc.rs"]
pub mod soc;

use crate::pal::hal::{
    HardwareAuthoritySet, HardwareBase, HardwareCpuCaps, HardwareCpuDescription, HardwareCpuQuery,
    HardwareCpuSupport, HardwareCpuVendor, HardwareError, HardwareGuarantee,
    HardwareImplementationKind, HardwareSimdSet, HardwareStackAbi, HardwareSupport,
    HardwareTopologyNodeId, HardwareTopologyQuery, HardwareTopologySummary, HardwareWriteSummary,
};
use crate::pal::thread::{ThreadClusterId, ThreadCoreClassId, ThreadCoreId, ThreadLogicalCpuId};

#[allow(unused_imports)]
pub use self::core::{CortexMCpuid, read_cpuid};
#[allow(unused_imports)]
pub use self::soc::board::{
    CortexMSocChipIdSupport, selected_soc_chip_id_support, selected_soc_name,
};

#[cfg(all(feature = "soc-rp2350", feature = "soc-stm32h7"))]
compile_error!("select at most one Cortex-M SoC feature for fusion-pal");

/// Selected Cortex-M hardware provider type.
#[derive(Debug, Clone, Copy, Default)]
pub struct CortexMHardware;

/// Target-selected Cortex-M hardware provider alias.
pub type PlatformHardware = CortexMHardware;

impl CortexMHardware {
    /// Creates a new Cortex-M hardware provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

/// Returns the selected Cortex-M hardware provider.
#[must_use]
pub const fn system_hardware() -> PlatformHardware {
    PlatformHardware::new()
}

impl HardwareBase for CortexMHardware {
    fn support(&self) -> HardwareSupport {
        let cpuid = read_cpuid();
        let vendor = cpu_vendor_guarantee(cpuid.vendor());

        HardwareSupport {
            cpu: HardwareCpuSupport {
                caps: HardwareCpuCaps::DESCRIPTOR
                    | HardwareCpuCaps::VENDOR
                    | HardwareCpuCaps::MEMORY_ORDERING
                    | HardwareCpuCaps::ATOMIC_WIDTHS
                    | HardwareCpuCaps::STACK_ABI,
                descriptor: HardwareGuarantee::Verified,
                vendor,
                cache_line_bytes: HardwareGuarantee::Unsupported,
                memory_ordering: HardwareGuarantee::Verified,
                atomic_widths: HardwareGuarantee::Verified,
                stack_abi: HardwareGuarantee::Verified,
                simd: HardwareGuarantee::Unsupported,
                authorities: HardwareAuthoritySet::ISA,
                implementation: HardwareImplementationKind::Native,
            },
            topology: soc::board::selected_soc().topology_support(),
        }
    }
}

impl HardwareCpuQuery for CortexMHardware {
    fn cpu_description(&self) -> Result<HardwareCpuDescription, HardwareError> {
        let cpuid = read_cpuid();

        Ok(HardwareCpuDescription {
            architecture: crate::hal::selected_architecture(),
            vendor: cpuid.vendor(),
            endianness: crate::hal::selected_endianness(),
            cache_line_bytes: None,
            memory_ordering: crate::hal::selected_memory_ordering(),
            pointer_width_bits: crate::hal::selected_pointer_width_bits(),
            atomic_widths: crate::hal::selected_atomic_widths(),
            simd: HardwareSimdSet::empty(),
        })
    }

    fn stack_abi(&self) -> Result<HardwareStackAbi, HardwareError> {
        Ok(core::stack_abi())
    }
}

impl HardwareTopologyQuery for CortexMHardware {
    fn topology_summary(&self) -> Result<HardwareTopologySummary, HardwareError> {
        soc::board::topology_summary()
    }

    fn write_logical_cpus(
        &self,
        output: &mut [ThreadLogicalCpuId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        soc::board::write_logical_cpus(output)
    }

    fn write_cores(
        &self,
        output: &mut [ThreadCoreId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        soc::board::write_cores(output)
    }

    fn write_clusters(
        &self,
        output: &mut [ThreadClusterId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        let _ = output;
        Err(HardwareError::unsupported())
    }

    fn write_packages(
        &self,
        output: &mut [HardwareTopologyNodeId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        let _ = output;
        Err(HardwareError::unsupported())
    }

    fn write_numa_nodes(
        &self,
        output: &mut [HardwareTopologyNodeId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        let _ = output;
        Err(HardwareError::unsupported())
    }

    fn write_core_classes(
        &self,
        output: &mut [ThreadCoreClassId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        let _ = output;
        Err(HardwareError::unsupported())
    }
}

const fn cpu_vendor_guarantee(vendor: HardwareCpuVendor) -> HardwareGuarantee {
    match vendor {
        HardwareCpuVendor::Unknown => HardwareGuarantee::Unknown,
        _ => HardwareGuarantee::Verified,
    }
}
