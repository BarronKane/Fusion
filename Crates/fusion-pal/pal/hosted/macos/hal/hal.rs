//! macOS fusion-pal hardware backend.
//!
//! This backend reports only machine facts it can surface honestly on Darwin through
//! architecture facts and `sysctlbyname` runtime queries.

use core::ffi::{
    c_char,
    c_void,
};
use core::mem::MaybeUninit;

use crate::contract::pal::cpu::{
    CachePadded64,
    fallback_stack_abi,
    selected_architecture,
    selected_atomic_widths,
    selected_cache_line_bytes,
    selected_endianness,
    selected_memory_ordering,
    selected_pointer_width_bits,
};
use crate::contract::pal::runtime::thread::{
    ThreadCoreId,
    ThreadLogicalCpuId,
    ThreadProcessorGroupId,
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
    HardwareTopologyCaps,
    HardwareTopologyNodeId,
    HardwareTopologyQueryContract,
    HardwareTopologySummary,
    HardwareTopologySupport,
    HardwareWriteSummary,
};

/// Selected macOS hardware provider type.
#[derive(Debug, Clone, Copy, Default)]
pub struct MacOsHardware;

/// Target-selected macOS hardware provider alias.
pub type PlatformHardware = MacOsHardware;

/// Compile-time cache-padding wrapper for macOS-hosted builds.
pub type PlatformCachePadded<T> = CachePadded64<T>;

/// Compile-time cache-padding alignment exported by the selected macOS backend.
pub const PLATFORM_CACHE_LINE_ALIGN_BYTES: usize = 64;

impl MacOsHardware {
    /// Creates a new macOS hardware provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

/// Returns the selected macOS hardware provider.
#[must_use]
pub const fn system_hardware() -> PlatformHardware {
    PlatformHardware::new()
}

impl HardwareBaseContract for MacOsHardware {
    fn support(&self) -> HardwareSupport {
        support()
    }
}

impl HardwareCpuQueryContract for MacOsHardware {
    fn cpu_description(&self) -> Result<HardwareCpuDescription, HardwareError> {
        Ok(cpu_description())
    }

    fn stack_abi(&self) -> Result<HardwareStackAbi, HardwareError> {
        Ok(fallback_stack_abi())
    }
}

impl HardwareTopologyQueryContract for MacOsHardware {
    fn topology_summary(&self) -> Result<HardwareTopologySummary, HardwareError> {
        topology_summary()
    }

    fn write_logical_cpus(
        &self,
        output: &mut [ThreadLogicalCpuId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        write_logical_cpus(output)
    }

    fn write_cores(
        &self,
        output: &mut [ThreadCoreId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        write_cores(output)
    }

    fn write_clusters(
        &self,
        _output: &mut [crate::contract::pal::runtime::thread::ThreadClusterId],
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
        _output: &mut [crate::contract::pal::runtime::thread::ThreadCoreClassId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        Err(HardwareError::unsupported())
    }
}

fn support() -> HardwareSupport {
    let vendor = runtime_vendor_guarantee();
    let cache_line_bytes = runtime_cache_line_guarantee();
    let simd = runtime_simd_guarantee();
    let topology = topology_support();

    let mut cpu_caps = HardwareCpuCaps::DESCRIPTOR
        | HardwareCpuCaps::MEMORY_ORDERING
        | HardwareCpuCaps::ATOMIC_WIDTHS
        | HardwareCpuCaps::STACK_ABI;

    if vendor != HardwareGuarantee::Unsupported {
        cpu_caps |= HardwareCpuCaps::VENDOR;
    }
    if cache_line_bytes != HardwareGuarantee::Unsupported {
        cpu_caps |= HardwareCpuCaps::CACHE_LINE_BYTES;
    }
    if simd != HardwareGuarantee::Unsupported {
        cpu_caps |= HardwareCpuCaps::SIMD;
    }

    HardwareSupport {
        cpu: HardwareCpuSupport {
            caps: cpu_caps,
            descriptor: HardwareGuarantee::Verified,
            vendor,
            cache_line_bytes,
            memory_ordering: HardwareGuarantee::Verified,
            atomic_widths: HardwareGuarantee::Verified,
            stack_abi: HardwareGuarantee::Verified,
            simd,
            authorities: cpu_authorities(cache_line_bytes, simd),
            implementation: HardwareImplementationKind::Native,
        },
        topology,
    }
}

fn cpu_description() -> HardwareCpuDescription {
    HardwareCpuDescription {
        architecture: selected_architecture(),
        vendor: runtime_vendor(),
        endianness: selected_endianness(),
        cache_line_bytes: runtime_cache_line_bytes().or(selected_cache_line_bytes()),
        memory_ordering: selected_memory_ordering(),
        pointer_width_bits: selected_pointer_width_bits(),
        atomic_widths: selected_atomic_widths(),
        simd: runtime_simd_set(),
    }
}

fn topology_summary() -> Result<HardwareTopologySummary, HardwareError> {
    let logical_cpu_count = runtime_logical_cpu_count().ok();
    let core_count = runtime_core_count().ok();

    if logical_cpu_count.is_none() && core_count.is_none() {
        return Err(HardwareError::unsupported());
    }

    Ok(HardwareTopologySummary {
        logical_cpu_count,
        core_count,
        cluster_count: None,
        package_count: None,
        numa_node_count: None,
        core_class_count: None,
    })
}

fn write_logical_cpus(
    output: &mut [ThreadLogicalCpuId],
) -> Result<HardwareWriteSummary, HardwareError> {
    let total = runtime_logical_cpu_count()?;
    let mut written = 0usize;

    for index in 0..total {
        let index_u16 =
            u16::try_from(index).map_err(|_| HardwareError::platform(libc::EOVERFLOW))?;
        if written < output.len() {
            output[written] = ThreadLogicalCpuId {
                group: ThreadProcessorGroupId(0),
                index: index_u16,
            };
            written += 1;
        }
    }

    Ok(HardwareWriteSummary::new(total, written))
}

fn write_cores(output: &mut [ThreadCoreId]) -> Result<HardwareWriteSummary, HardwareError> {
    let total = runtime_core_count()?;
    let mut written = 0usize;

    for index in 0..total {
        let index_u32 =
            u32::try_from(index).map_err(|_| HardwareError::platform(libc::EOVERFLOW))?;
        if written < output.len() {
            output[written] = ThreadCoreId(index_u32);
            written += 1;
        }
    }

    Ok(HardwareWriteSummary::new(total, written))
}

fn cpu_authorities(
    cache_line_bytes: HardwareGuarantee,
    simd: HardwareGuarantee,
) -> HardwareAuthoritySet {
    let mut authorities = HardwareAuthoritySet::ISA;

    if cache_line_bytes != HardwareGuarantee::Unsupported || simd != HardwareGuarantee::Unsupported
    {
        authorities |= HardwareAuthoritySet::OPERATING_SYSTEM;
    }

    authorities
}

fn topology_support() -> HardwareTopologySupport {
    let logical = runtime_logical_cpu_count().is_ok();
    let cores = runtime_core_count().is_ok();

    let mut caps = HardwareTopologyCaps::empty();
    let mut summary = HardwareGuarantee::Unsupported;
    let mut logical_g = HardwareGuarantee::Unsupported;
    let mut core_g = HardwareGuarantee::Unsupported;

    if logical {
        caps |= HardwareTopologyCaps::SUMMARY | HardwareTopologyCaps::LOGICAL_CPUS;
        summary = HardwareGuarantee::Verified;
        logical_g = HardwareGuarantee::Verified;
    }
    if cores {
        caps |= HardwareTopologyCaps::SUMMARY | HardwareTopologyCaps::CORES;
        summary = HardwareGuarantee::Verified;
        core_g = HardwareGuarantee::Verified;
    }

    if caps.is_empty() {
        return HardwareTopologySupport::unsupported();
    }

    HardwareTopologySupport {
        caps,
        summary,
        logical_cpus: logical_g,
        cores: core_g,
        clusters: HardwareGuarantee::Unsupported,
        packages: HardwareGuarantee::Unsupported,
        numa_nodes: HardwareGuarantee::Unsupported,
        core_classes: HardwareGuarantee::Unsupported,
        authorities: HardwareAuthoritySet::OPERATING_SYSTEM | HardwareAuthoritySet::TOPOLOGY,
        implementation: HardwareImplementationKind::Native,
    }
}

fn runtime_logical_cpu_count() -> Result<usize, HardwareError> {
    let logical = sysctl_u32(b"hw.logicalcpu\0")?;
    usize::try_from(logical).map_err(|_| HardwareError::platform(libc::EOVERFLOW))
}

fn runtime_core_count() -> Result<usize, HardwareError> {
    let cores = sysctl_u32(b"hw.physicalcpu\0")?;
    usize::try_from(cores).map_err(|_| HardwareError::platform(libc::EOVERFLOW))
}

fn runtime_cache_line_bytes() -> Option<usize> {
    sysctl_u32(b"hw.cachelinesize\0")
        .ok()
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value != 0)
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn runtime_vendor() -> HardwareCpuVendor {
    #[cfg(target_arch = "x86")]
    use core::arch::x86::__cpuid;
    #[cfg(target_arch = "x86_64")]
    use core::arch::x86_64::__cpuid;

    let leaf0 = unsafe { __cpuid(0) };
    let mut vendor = [0u8; 12];
    vendor[0..4].copy_from_slice(&leaf0.ebx.to_le_bytes());
    vendor[4..8].copy_from_slice(&leaf0.edx.to_le_bytes());
    vendor[8..12].copy_from_slice(&leaf0.ecx.to_le_bytes());

    match &vendor {
        b"GenuineIntel" => HardwareCpuVendor::Intel,
        b"AuthenticAMD" => HardwareCpuVendor::Amd,
        _ => HardwareCpuVendor::Other,
    }
}

#[cfg(target_arch = "aarch64")]
const fn runtime_vendor() -> HardwareCpuVendor {
    // macOS `aarch64` targets are Apple silicon in supported configurations.
    HardwareCpuVendor::Apple
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")))]
const fn runtime_vendor() -> HardwareCpuVendor {
    HardwareCpuVendor::Unknown
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64"))]
const fn runtime_vendor_guarantee() -> HardwareGuarantee {
    HardwareGuarantee::Verified
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")))]
const fn runtime_vendor_guarantee() -> HardwareGuarantee {
    HardwareGuarantee::Unsupported
}

fn runtime_cache_line_guarantee() -> HardwareGuarantee {
    if runtime_cache_line_bytes().is_some() || selected_cache_line_bytes().is_some() {
        HardwareGuarantee::Verified
    } else {
        HardwareGuarantee::Unsupported
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64"))]
const fn runtime_simd_guarantee() -> HardwareGuarantee {
    HardwareGuarantee::Verified
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")))]
const fn runtime_simd_guarantee() -> HardwareGuarantee {
    HardwareGuarantee::Unsupported
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn runtime_simd_set() -> HardwareSimdSet {
    #[cfg(target_arch = "x86")]
    use core::arch::x86::{
        __cpuid,
        __cpuid_count,
        _xgetbv,
    };
    #[cfg(target_arch = "x86_64")]
    use core::arch::x86_64::{
        __cpuid,
        __cpuid_count,
        _xgetbv,
    };

    let leaf0 = unsafe { __cpuid(0) };
    if leaf0.eax < 1 {
        return HardwareSimdSet::empty();
    }

    let leaf1 = unsafe { __cpuid(1) };
    let mut simd = HardwareSimdSet::empty();

    if bit32(leaf1.edx, 25) {
        simd |= HardwareSimdSet::SSE;
    }
    if bit32(leaf1.edx, 26) {
        simd |= HardwareSimdSet::SSE2;
    }
    if bit32(leaf1.ecx, 0) {
        simd |= HardwareSimdSet::SSE3;
    }
    if bit32(leaf1.ecx, 9) {
        simd |= HardwareSimdSet::SSSE3;
    }
    if bit32(leaf1.ecx, 19) {
        simd |= HardwareSimdSet::SSE4_1;
    }
    if bit32(leaf1.ecx, 20) {
        simd |= HardwareSimdSet::SSE4_2;
    }

    let xcr0 = if bit32(leaf1.ecx, 26) && bit32(leaf1.ecx, 27) {
        Some(unsafe { _xgetbv(0) })
    } else {
        None
    };
    let os_avx = xcr0.is_some_and(|value| value & 0b110 == 0b110);
    let os_avx512 = xcr0.is_some_and(|value| value & 0xe6 == 0xe6);

    if bit32(leaf1.ecx, 28) && os_avx {
        simd |= HardwareSimdSet::AVX;
    }

    if leaf0.eax >= 7 {
        let leaf7 = unsafe { __cpuid_count(7, 0) };
        if bit32(leaf7.ebx, 5) && os_avx {
            simd |= HardwareSimdSet::AVX2;
        }
        if bit32(leaf7.ebx, 16) && os_avx512 {
            simd |= HardwareSimdSet::AVX512F;
        }
    }

    simd
}

#[cfg(target_arch = "aarch64")]
const fn runtime_simd_set() -> HardwareSimdSet {
    // Advanced SIMD (NEON) is mandatory in AArch64 user space.
    HardwareSimdSet::NEON
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")))]
const fn runtime_simd_set() -> HardwareSimdSet {
    HardwareSimdSet::empty()
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
const fn bit32(value: u32, bit: u32) -> bool {
    (value & (1u32 << bit)) != 0
}

fn sysctl_u32(name: &[u8]) -> Result<u32, HardwareError> {
    let mut value = MaybeUninit::<u32>::uninit();
    let mut len = core::mem::size_of::<u32>();
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr().cast::<c_char>(),
            value.as_mut_ptr().cast::<c_void>(),
            &raw mut len,
            core::ptr::null_mut(),
            0,
        )
    };

    if rc != 0 {
        return Err(map_errno(last_errno()));
    }
    if len != core::mem::size_of::<u32>() {
        return Err(HardwareError::invalid());
    }

    Ok(unsafe { value.assume_init() })
}

const fn map_errno(errno: libc::c_int) -> HardwareError {
    match errno {
        libc::ENOMEM => HardwareError::resource_exhausted(),
        libc::EINVAL => HardwareError::invalid(),
        libc::EBUSY => HardwareError::busy(),
        libc::ENOENT | libc::ENOTSUP | libc::EOPNOTSUPP => HardwareError::unsupported(),
        libc::EEXIST => HardwareError::state_conflict(),
        _ => HardwareError::platform(errno),
    }
}

fn last_errno() -> libc::c_int {
    unsafe { *libc::__error() }
}

#[cfg(all(test, feature = "std", not(target_os = "none")))]
mod tests {
    extern crate std;

    use super::*;

    #[test]
    fn macos_hardware_support_reports_native_cpu_surface() {
        let support = system_hardware().support();
        assert_eq!(
            support.cpu.implementation,
            HardwareImplementationKind::Native
        );
        assert_eq!(support.cpu.descriptor, HardwareGuarantee::Verified);
        assert_eq!(support.cpu.memory_ordering, HardwareGuarantee::Verified);
        assert_eq!(support.cpu.atomic_widths, HardwareGuarantee::Verified);
        assert_eq!(support.cpu.stack_abi, HardwareGuarantee::Verified);
        assert!(support.cpu.caps.contains(HardwareCpuCaps::DESCRIPTOR));
        assert!(support.cpu.caps.contains(HardwareCpuCaps::MEMORY_ORDERING));
        assert!(support.cpu.caps.contains(HardwareCpuCaps::ATOMIC_WIDTHS));
        assert!(support.cpu.caps.contains(HardwareCpuCaps::STACK_ABI));
    }

    #[test]
    fn macos_hardware_topology_summary_and_writes_are_consistent_when_available() {
        let hardware = system_hardware();
        let summary = hardware.topology_summary();

        let Ok(summary) = summary else {
            return;
        };

        if let Some(total) = summary.logical_cpu_count {
            let mut ids = self::std::vec![
                ThreadLogicalCpuId {
                    group: ThreadProcessorGroupId(0),
                    index: 0,
                };
                total
            ];
            let written = hardware
                .write_logical_cpus(&mut ids)
                .expect("logical cpu write should succeed");
            assert_eq!(written.total, total);
            assert_eq!(written.written, total);
        }

        if let Some(total) = summary.core_count {
            let mut cores = self::std::vec![ThreadCoreId(0); total];
            let written = hardware
                .write_cores(&mut cores)
                .expect("core write should succeed");
            assert_eq!(written.total, total);
            assert_eq!(written.written, total);
        }
    }
}
