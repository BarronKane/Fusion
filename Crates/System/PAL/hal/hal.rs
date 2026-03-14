//! Selected hardware abstraction surface.
//!
//! This module sits beside [`crate::sys`]: it consumes selected platform exports where they
//! already surface truthful hardware-adjacent facts, fills in compile-time ISA and ABI
//! truth where that is the only honest source today, and exposes a single provider
//! through the backend-neutral contracts in [`crate::pal::hal`].

use crate::pal::context::{ContextBase as _, ContextImplementationKind};
use crate::pal::thread::{ThreadClusterId, ThreadCoreClassId, ThreadCoreId, ThreadLogicalCpuId};

/// Re-export the backend-neutral HAL vocabulary from the selected hardware surface.
pub use crate::pal::hal::*;

/// Selected hardware-query provider for the current target.
#[derive(Debug, Clone, Copy, Default)]
pub struct HardwareSystem;

impl HardwareSystem {
    /// Creates a new selected hardware-query provider.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

/// Returns the selected hardware-query provider for the current target.
#[must_use]
pub const fn system_hardware() -> HardwareSystem {
    HardwareSystem::new()
}

impl HardwareBase for HardwareSystem {
    fn support(&self) -> HardwareSupport {
        HardwareSupport {
            cpu: HardwareCpuSupport {
                caps: HardwareCpuCaps::DESCRIPTOR
                    | cache_line_size_cap()
                    | HardwareCpuCaps::MEMORY_ORDERING
                    | HardwareCpuCaps::ATOMIC_WIDTHS
                    | HardwareCpuCaps::STACK_ABI,
                descriptor: HardwareGuarantee::Verified,
                vendor: HardwareGuarantee::Unsupported,
                cache_line_bytes: cache_line_size_guarantee(),
                memory_ordering: HardwareGuarantee::Verified,
                atomic_widths: HardwareGuarantee::Verified,
                stack_abi: stack_abi_guarantee(),
                authorities: HardwareAuthoritySet::ISA | HardwareAuthoritySet::OPERATING_SYSTEM,
                implementation: HardwareImplementationKind::Native,
            },
            topology: HardwareTopologySupport::unsupported(),
        }
    }
}

impl HardwareCpuQuery for HardwareSystem {
    fn cpu_description(&self) -> Result<HardwareCpuDescription, HardwareError> {
        Ok(HardwareCpuDescription {
            architecture: selected_architecture(),
            vendor: HardwareCpuVendor::Unknown,
            endianness: selected_endianness(),
            cache_line_bytes: selected_cache_line_bytes(),
            memory_ordering: selected_memory_ordering(),
            pointer_width_bits: selected_pointer_width_bits(),
            atomic_widths: selected_atomic_widths(),
        })
    }

    fn stack_abi(&self) -> Result<HardwareStackAbi, HardwareError> {
        let context_support = crate::sys::context::system_context().support();
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

impl HardwareTopologyQuery for HardwareSystem {
    fn topology_summary(&self) -> Result<HardwareTopologySummary, HardwareError> {
        Err(HardwareError::unsupported())
    }

    fn write_logical_cpus(
        &self,
        _output: &mut [ThreadLogicalCpuId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        Err(HardwareError::unsupported())
    }

    fn write_cores(
        &self,
        _output: &mut [ThreadCoreId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        Err(HardwareError::unsupported())
    }

    fn write_clusters(
        &self,
        _output: &mut [ThreadClusterId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        Err(HardwareError::unsupported())
    }

    fn write_packages(
        &self,
        _output: &mut [HardwareTopologyNodeId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        Err(HardwareError::unsupported())
    }

    fn write_numa_nodes(
        &self,
        _output: &mut [HardwareTopologyNodeId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        Err(HardwareError::unsupported())
    }

    fn write_core_classes(
        &self,
        _output: &mut [ThreadCoreClassId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        Err(HardwareError::unsupported())
    }
}

fn stack_abi_guarantee() -> HardwareGuarantee {
    let context_support = crate::sys::context::system_context().support();
    if context_support.implementation != ContextImplementationKind::Unsupported {
        return context_support.guarantee.into();
    }

    HardwareGuarantee::Verified
}

const fn cache_line_size_cap() -> HardwareCpuCaps {
    if selected_cache_line_bytes().is_some() {
        HardwareCpuCaps::CACHE_LINE_BYTES
    } else {
        HardwareCpuCaps::empty()
    }
}

const fn cache_line_size_guarantee() -> HardwareGuarantee {
    if selected_cache_line_bytes().is_some() {
        HardwareGuarantee::Verified
    } else {
        HardwareGuarantee::Unsupported
    }
}

fn selected_atomic_widths() -> HardwareAtomicWidthSet {
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

const fn selected_architecture() -> HardwareCpuArchitecture {
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

const fn selected_endianness() -> HardwareEndian {
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

const fn selected_memory_ordering() -> HardwareMemoryOrdering {
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

const fn selected_cache_line_bytes() -> Option<usize> {
    #[cfg(target_arch = "x86_64")]
    {
        return Some(64);
    }

    #[cfg(any(target_arch = "arm", target_arch = "riscv64"))]
    {
        return Some(64);
    }

    #[cfg(target_arch = "aarch64")]
    {
        return None;
    }

    #[allow(unreachable_code)]
    None
}

const fn selected_pointer_width_bits() -> u16 {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_hardware_reports_cpu_support() {
        let hardware = system_hardware();
        let support = hardware.support();

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
    fn system_hardware_reports_architecture_and_stack_abi() {
        let hardware = system_hardware();
        let cpu = hardware.cpu_description().expect("cpu description");
        let stack = hardware.stack_abi().expect("stack abi");

        #[cfg(target_arch = "x86_64")]
        {
            assert_eq!(cpu.architecture, HardwareCpuArchitecture::X86_64);
            assert_eq!(cpu.cache_line_bytes, Some(64));
            assert_eq!(stack.red_zone_bytes, 128);
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
    fn system_hardware_topology_is_honestly_unsupported_for_now() {
        let hardware = system_hardware();

        assert_eq!(
            hardware.support().topology.implementation,
            HardwareImplementationKind::Unsupported
        );
        assert_eq!(
            hardware
                .topology_summary()
                .expect_err("topology should be unsupported")
                .kind(),
            HardwareErrorKind::Unsupported
        );
    }
}
