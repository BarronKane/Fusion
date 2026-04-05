//! Cortex-M hardware backend.

pub mod core;
#[path = "soc/soc.rs"]
pub mod soc;

use crate::contract::pal::cpu::{
    CachePadded32,
    selected_architecture,
    selected_atomic_widths,
    selected_endianness,
    selected_memory_ordering,
    selected_pointer_width_bits,
};
use crate::contract::pal::runtime::thread::{
    ThreadClusterId,
    ThreadCoreClassId,
    ThreadCoreId,
    ThreadLogicalCpuId,
};
use crate::contract::pal::{
    HardwareAuthoritySet,
    HardwareBaseContract,
    HardwareCpuCaps,
    HardwareCpuDescription,
    HardwareCpuQueryContract,
    HardwareCpuSupport,
    HardwareCpuVendor,
    HardwareError,
    HardwareGuarantee,
    HardwareImplementationKind,
    HardwareSimdSet,
    HardwareStackAbi,
    HardwareSupport,
    HardwareTopologyNodeId,
    HardwareTopologyQueryContract,
    HardwareTopologySummary,
    HardwareWriteSummary,
};
#[allow(unused_imports)]
pub use self::core::{
    CortexMCpuid,
    read_cpuid,
};
#[allow(unused_imports)]
pub use self::soc::board::{
    CortexMExceptionStackObservation,
    CortexMIrqClass,
    CortexMIrqDescriptor,
    CortexMSocChipIdSupport,
    CortexMSocChipIdentity,
    CortexMSocDeviceIdSupport,
    CortexMSocDeviceIdentity,
    chip_identity as selected_soc_chip_identity,
    device_identity as selected_soc_device_identity,
    enter_power_mode as selected_soc_enter_power_mode,
    exception_stack_observation as selected_soc_exception_stack_observation,
    inline_current_exception_stack_allows as selected_soc_inline_current_exception_stack_allows,
    irq_acknowledge as selected_soc_irq_acknowledge,
    irq_acknowledge_supported as selected_soc_irq_acknowledge_supported,
    irq_clear_pending as selected_soc_irq_clear_pending,
    irq_disable as selected_soc_irq_disable,
    irq_enable as selected_soc_irq_enable,
    irq_implemented_priority_bits as selected_soc_irq_implemented_priority_bits,
    irq_priority_supported as selected_soc_irq_priority_supported,
    irq_set_pending as selected_soc_irq_set_pending,
    irq_set_priority as selected_soc_irq_set_priority,
    irqs as selected_soc_irqs,
    selected_soc_chip_id_support,
    selected_soc_device_id_support,
    selected_soc_name,
};

/// Selected Cortex-M hardware provider type.
#[derive(Debug, Clone, Copy, Default)]
pub struct CortexMHardware;

/// Target-selected Cortex-M hardware provider alias.
pub type PlatformHardware = CortexMHardware;

/// Compile-time cache-padding wrapper for Cortex-M builds.
pub type PlatformCachePadded<T> = CachePadded32<T>;

/// Compile-time cache-padding alignment exported by the selected Cortex-M backend.
pub const PLATFORM_CACHE_LINE_ALIGN_BYTES: usize = 32;

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

impl HardwareBaseContract for CortexMHardware {
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

impl HardwareCpuQueryContract for CortexMHardware {
    fn cpu_description(&self) -> Result<HardwareCpuDescription, HardwareError> {
        let cpuid = read_cpuid();

        Ok(HardwareCpuDescription {
            architecture: selected_architecture(),
            vendor: cpuid.vendor(),
            endianness: selected_endianness(),
            cache_line_bytes: None,
            memory_ordering: selected_memory_ordering(),
            pointer_width_bits: selected_pointer_width_bits(),
            atomic_widths: selected_atomic_widths(),
            simd: HardwareSimdSet::empty(),
        })
    }

    fn stack_abi(&self) -> Result<HardwareStackAbi, HardwareError> {
        Ok(core::stack_abi())
    }
}

impl HardwareTopologyQueryContract for CortexMHardware {
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
        soc::board::write_clusters(output)
    }

    fn write_packages(
        &self,
        output: &mut [HardwareTopologyNodeId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        soc::board::write_packages(output)
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
        soc::board::write_core_classes(output)
    }
}

const fn cpu_vendor_guarantee(vendor: HardwareCpuVendor) -> HardwareGuarantee {
    match vendor {
        HardwareCpuVendor::Unknown => HardwareGuarantee::Unknown,
        _ => HardwareGuarantee::Verified,
    }
}
