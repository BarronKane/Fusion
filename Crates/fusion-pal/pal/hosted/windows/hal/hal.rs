//! Windows fusion-pal hardware backend.
//!
//! This backend reports only machine facts it can surface honestly through Win32 topology
//! queries and ISA runtime probes. When the hosted process is ISA-emulated, ISA-derived CPU
//! truth is rejected rather than reported as native hardware truth.

use core::mem::size_of;
use core::slice;

use std::vec::Vec;

use windows::Win32::Foundation::{
    ERROR_BUSY,
    ERROR_INSUFFICIENT_BUFFER,
    ERROR_INVALID_PARAMETER,
    ERROR_NOT_ENOUGH_MEMORY,
    ERROR_NOT_SUPPORTED,
    GetLastError,
    WIN32_ERROR,
};
use windows::Win32::System::SystemInformation::{
    CacheData,
    CacheUnified,
    GetLogicalProcessorInformationEx,
    GROUP_RELATIONSHIP,
    IMAGE_FILE_MACHINE,
    IMAGE_FILE_MACHINE_UNKNOWN,
    LOGICAL_PROCESSOR_RELATIONSHIP,
    PROCESSOR_GROUP_INFO,
    RelationCache,
    RelationGroup,
    RelationNumaNode,
    RelationProcessorCore,
    SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess,
    IsWow64Process2,
};

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

/// Selected Windows hardware provider type.
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsHardware;

/// Target-selected Windows hardware provider alias.
pub type PlatformHardware = WindowsHardware;

/// Compile-time cache-padding wrapper for Windows-hosted builds.
pub type PlatformCachePadded<T> = CachePadded64<T>;

/// Compile-time cache-padding alignment exported by the selected Windows backend.
pub const PLATFORM_CACHE_LINE_ALIGN_BYTES: usize = 64;

impl WindowsHardware {
    /// Creates a new Windows hardware provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

/// Returns the selected Windows hardware provider.
#[must_use]
pub const fn system_hardware() -> PlatformHardware {
    PlatformHardware::new()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CpuTruthMode {
    Native,
    Emulated,
    Unknown,
}

impl HardwareBaseContract for WindowsHardware {
    fn support(&self) -> HardwareSupport {
        support()
    }
}

impl HardwareCpuQueryContract for WindowsHardware {
    fn cpu_description(&self) -> Result<HardwareCpuDescription, HardwareError> {
        if cpu_truth_mode() != CpuTruthMode::Native {
            return Err(HardwareError::unsupported());
        }

        Ok(cpu_description())
    }

    fn stack_abi(&self) -> Result<HardwareStackAbi, HardwareError> {
        if cpu_truth_mode() != CpuTruthMode::Native {
            return Err(HardwareError::unsupported());
        }

        Ok(fallback_stack_abi())
    }
}

impl HardwareTopologyQueryContract for WindowsHardware {
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
        output: &mut [HardwareTopologyNodeId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        write_numa_nodes(output)
    }

    fn write_core_classes(
        &self,
        _output: &mut [crate::contract::pal::runtime::thread::ThreadCoreClassId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        Err(HardwareError::unsupported())
    }
}

fn support() -> HardwareSupport {
    let topology = topology_support();

    let cpu = match cpu_truth_mode() {
        CpuTruthMode::Native => {
            let vendor = runtime_vendor_guarantee();
            let cache_line_bytes = runtime_cache_line_guarantee();
            let simd = runtime_simd_guarantee();

            let mut caps = HardwareCpuCaps::DESCRIPTOR
                | HardwareCpuCaps::MEMORY_ORDERING
                | HardwareCpuCaps::ATOMIC_WIDTHS
                | HardwareCpuCaps::STACK_ABI;

            if vendor != HardwareGuarantee::Unsupported {
                caps |= HardwareCpuCaps::VENDOR;
            }
            if cache_line_bytes != HardwareGuarantee::Unsupported {
                caps |= HardwareCpuCaps::CACHE_LINE_BYTES;
            }
            if simd != HardwareGuarantee::Unsupported {
                caps |= HardwareCpuCaps::SIMD;
            }

            HardwareCpuSupport {
                caps,
                descriptor: HardwareGuarantee::Verified,
                vendor,
                cache_line_bytes,
                memory_ordering: HardwareGuarantee::Verified,
                atomic_widths: HardwareGuarantee::Verified,
                stack_abi: HardwareGuarantee::Verified,
                simd,
                authorities: cpu_authorities(cache_line_bytes, simd),
                implementation: HardwareImplementationKind::Native,
            }
        }
        CpuTruthMode::Emulated => HardwareCpuSupport {
            implementation: HardwareImplementationKind::Emulated,
            ..HardwareCpuSupport::unsupported()
        },
        CpuTruthMode::Unknown => HardwareCpuSupport::unsupported(),
    };

    HardwareSupport { cpu, topology }
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
    let numa_node_count = runtime_numa_node_count().ok();

    if logical_cpu_count.is_none() && core_count.is_none() && numa_node_count.is_none() {
        return Err(HardwareError::unsupported());
    }

    Ok(HardwareTopologySummary {
        logical_cpu_count,
        core_count,
        cluster_count: None,
        package_count: None,
        numa_node_count,
        core_class_count: None,
    })
}

fn write_logical_cpus(
    output: &mut [ThreadLogicalCpuId],
) -> Result<HardwareWriteSummary, HardwareError> {
    let buffer = query_processor_information(RelationGroup)?;
    let mut total = 0usize;
    let mut written = 0usize;
    let mut next_group_index = 0usize;

    for_each_record(&buffer, |record| {
        if record.Relationship != RelationGroup {
            return Err(HardwareError::invalid());
        }

        let relation = unsafe { &record.Anonymous.Group };
        let groups = unsafe { group_infos(relation) };

        for group in groups {
            let group_id = ThreadProcessorGroupId(
                u16::try_from(next_group_index).map_err(|_| HardwareError::resource_exhausted())?,
            );
            write_group_logical_cpus(
                group_id,
                group.ActiveProcessorMask,
                output,
                &mut total,
                &mut written,
            )?;
            next_group_index += 1;
        }

        Ok(())
    })?;

    if total == 0 {
        return Err(HardwareError::unsupported());
    }

    Ok(HardwareWriteSummary::new(total, written))
}

fn write_cores(output: &mut [ThreadCoreId]) -> Result<HardwareWriteSummary, HardwareError> {
    let total = runtime_core_count()?;
    let mut written = 0usize;

    for index in 0..total {
        let index_u32 = u32::try_from(index).map_err(|_| HardwareError::resource_exhausted())?;
        if written < output.len() {
            output[written] = ThreadCoreId(index_u32);
            written += 1;
        }
    }

    Ok(HardwareWriteSummary::new(total, written))
}

fn write_numa_nodes(
    output: &mut [HardwareTopologyNodeId],
) -> Result<HardwareWriteSummary, HardwareError> {
    let buffer = query_processor_information(RelationNumaNode)?;
    let mut total = 0usize;
    let mut written = 0usize;

    for_each_record(&buffer, |record| {
        if record.Relationship != RelationNumaNode {
            return Err(HardwareError::invalid());
        }

        let relation = unsafe { &record.Anonymous.NumaNode };
        if written < output.len() {
            output[written] = crate::contract::pal::mem::MemTopologyNodeId(relation.NodeNumber);
            written += 1;
        }
        total += 1;
        Ok(())
    })?;

    if total == 0 {
        return Err(HardwareError::unsupported());
    }

    Ok(HardwareWriteSummary::new(total, written))
}

fn topology_support() -> HardwareTopologySupport {
    let logical = runtime_logical_cpu_count().is_ok();
    let cores = runtime_core_count().is_ok();
    let numa_nodes = runtime_numa_node_count().is_ok();

    let mut caps = HardwareTopologyCaps::empty();
    let mut summary = HardwareGuarantee::Unsupported;
    let mut logical_g = HardwareGuarantee::Unsupported;
    let mut core_g = HardwareGuarantee::Unsupported;
    let mut numa_g = HardwareGuarantee::Unsupported;

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
    if numa_nodes {
        caps |= HardwareTopologyCaps::SUMMARY | HardwareTopologyCaps::NUMA_NODES;
        summary = HardwareGuarantee::Verified;
        numa_g = HardwareGuarantee::Verified;
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
        numa_nodes: numa_g,
        core_classes: HardwareGuarantee::Unsupported,
        authorities: HardwareAuthoritySet::OPERATING_SYSTEM | HardwareAuthoritySet::TOPOLOGY,
        implementation: HardwareImplementationKind::Native,
    }
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

fn runtime_logical_cpu_count() -> Result<usize, HardwareError> {
    let buffer = query_processor_information(RelationGroup)?;
    let mut total = 0usize;

    for_each_record(&buffer, |record| {
        if record.Relationship != RelationGroup {
            return Err(HardwareError::invalid());
        }

        let relation = unsafe { &record.Anonymous.Group };
        let groups = unsafe { group_infos(relation) };
        for group in groups {
            total = total
                .checked_add(usize::from(group.ActiveProcessorCount))
                .ok_or_else(HardwareError::resource_exhausted)?;
        }
        Ok(())
    })?;

    if total == 0 {
        Err(HardwareError::unsupported())
    } else {
        Ok(total)
    }
}

fn runtime_core_count() -> Result<usize, HardwareError> {
    count_records(RelationProcessorCore)
}

fn runtime_numa_node_count() -> Result<usize, HardwareError> {
    count_records(RelationNumaNode)
}

fn count_records(relationship: LOGICAL_PROCESSOR_RELATIONSHIP) -> Result<usize, HardwareError> {
    let buffer = query_processor_information(relationship)?;
    let mut total = 0usize;

    for_each_record(&buffer, |record| {
        if record.Relationship != relationship {
            return Err(HardwareError::invalid());
        }
        total += 1;
        Ok(())
    })?;

    if total == 0 {
        Err(HardwareError::unsupported())
    } else {
        Ok(total)
    }
}

fn query_processor_information(
    relationship: LOGICAL_PROCESSOR_RELATIONSHIP,
) -> Result<Vec<u8>, HardwareError> {
    let mut bytes = 0u32;

    for _ in 0..4 {
        let _ = unsafe { GetLogicalProcessorInformationEx(relationship, None, &mut bytes) };
        let required = usize::try_from(bytes).map_err(|_| HardwareError::resource_exhausted())?;
        if required == 0 {
            return Err(map_win32_error(unsafe { GetLastError() }));
        }

        let mut buffer = vec![0u8; required];
        match unsafe {
            GetLogicalProcessorInformationEx(
                relationship,
                Some(
                    buffer
                        .as_mut_ptr()
                        .cast::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX>(),
                ),
                &mut bytes,
            )
        } {
            Ok(()) => {
                let written =
                    usize::try_from(bytes).map_err(|_| HardwareError::resource_exhausted())?;
                if written > buffer.len() {
                    return Err(HardwareError::invalid());
                }
                buffer.truncate(written);
                return Ok(buffer);
            }
            Err(_) => {
                let error = unsafe { GetLastError() };
                if error == ERROR_INSUFFICIENT_BUFFER {
                    continue;
                }
                return Err(map_win32_error(error));
            }
        }
    }

    Err(HardwareError::resource_exhausted())
}

fn for_each_record(
    buffer: &[u8],
    mut visitor: impl FnMut(&SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX) -> Result<(), HardwareError>,
) -> Result<(), HardwareError> {
    let mut offset = 0usize;

    while offset < buffer.len() {
        let remaining = buffer.len() - offset;
        if remaining < size_of::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX>() {
            return Err(HardwareError::invalid());
        }

        let record = unsafe {
            &*buffer[offset..]
                .as_ptr()
                .cast::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX>()
        };
        let size = usize::try_from(record.Size).map_err(|_| HardwareError::resource_exhausted())?;
        if size < size_of::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX>() || size > remaining {
            return Err(HardwareError::invalid());
        }

        visitor(record)?;
        offset += size;
    }

    Ok(())
}

fn write_group_logical_cpus(
    group: ThreadProcessorGroupId,
    mask: usize,
    output: &mut [ThreadLogicalCpuId],
    total: &mut usize,
    written: &mut usize,
) -> Result<(), HardwareError> {
    for bit in 0..usize::BITS {
        if (mask & (1usize << bit)) == 0 {
            continue;
        }

        let index = u16::try_from(bit).map_err(|_| HardwareError::resource_exhausted())?;
        if *written < output.len() {
            output[*written] = ThreadLogicalCpuId { group, index };
            *written += 1;
        }
        *total += 1;
    }

    Ok(())
}

unsafe fn group_infos(relation: &GROUP_RELATIONSHIP) -> &[PROCESSOR_GROUP_INFO] {
    unsafe {
        slice::from_raw_parts(
            relation.GroupInfo.as_ptr(),
            usize::from(relation.ActiveGroupCount),
        )
    }
}

fn runtime_cache_line_bytes() -> Option<usize> {
    let buffer = query_processor_information(RelationCache).ok()?;
    let mut cache_line_bytes = 0usize;

    if for_each_record(&buffer, |record| {
        if record.Relationship != RelationCache {
            return Err(HardwareError::invalid());
        }

        let cache = unsafe { &record.Anonymous.Cache };
        if (cache.Type == CacheData || cache.Type == CacheUnified) && cache.LineSize != 0 {
            cache_line_bytes = cache_line_bytes.max(usize::from(cache.LineSize));
        }

        Ok(())
    })
    .is_err()
    {
        return None;
    }

    (cache_line_bytes != 0).then_some(cache_line_bytes)
}

fn cpu_truth_mode() -> CpuTruthMode {
    let mut process_machine = IMAGE_FILE_MACHINE_UNKNOWN;
    let mut native_machine = IMAGE_FILE_MACHINE_UNKNOWN;

    if unsafe {
        IsWow64Process2(
            GetCurrentProcess(),
            &mut process_machine,
            Some(&mut native_machine),
        )
    }
    .is_err()
    {
        return CpuTruthMode::Unknown;
    }

    if native_machine == IMAGE_FILE_MACHINE_UNKNOWN {
        return CpuTruthMode::Unknown;
    }

    if native_machine_matches_target(native_machine) {
        CpuTruthMode::Native
    } else {
        CpuTruthMode::Emulated
    }
}

#[cfg(target_arch = "x86_64")]
fn native_machine_matches_target(native_machine: IMAGE_FILE_MACHINE) -> bool {
    native_machine == windows::Win32::System::SystemInformation::IMAGE_FILE_MACHINE_AMD64
}

#[cfg(target_arch = "x86")]
fn native_machine_matches_target(native_machine: IMAGE_FILE_MACHINE) -> bool {
    native_machine == windows::Win32::System::SystemInformation::IMAGE_FILE_MACHINE_I386
        || native_machine == windows::Win32::System::SystemInformation::IMAGE_FILE_MACHINE_AMD64
}

#[cfg(target_arch = "aarch64")]
fn native_machine_matches_target(native_machine: IMAGE_FILE_MACHINE) -> bool {
    native_machine == windows::Win32::System::SystemInformation::IMAGE_FILE_MACHINE_ARM64
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "x86", target_arch = "aarch64")))]
fn native_machine_matches_target(_native_machine: IMAGE_FILE_MACHINE) -> bool {
    false
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
    HardwareCpuVendor::Unknown
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")))]
const fn runtime_vendor() -> HardwareCpuVendor {
    HardwareCpuVendor::Unknown
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn runtime_vendor_guarantee() -> HardwareGuarantee {
    if cpu_truth_mode() == CpuTruthMode::Native {
        HardwareGuarantee::Verified
    } else {
        HardwareGuarantee::Unsupported
    }
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
const fn runtime_vendor_guarantee() -> HardwareGuarantee {
    HardwareGuarantee::Unsupported
}

fn runtime_cache_line_guarantee() -> HardwareGuarantee {
    if cpu_truth_mode() == CpuTruthMode::Native
        && (runtime_cache_line_bytes().is_some() || selected_cache_line_bytes().is_some())
    {
        HardwareGuarantee::Verified
    } else {
        HardwareGuarantee::Unsupported
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64"))]
fn runtime_simd_guarantee() -> HardwareGuarantee {
    if cpu_truth_mode() == CpuTruthMode::Native {
        HardwareGuarantee::Verified
    } else {
        HardwareGuarantee::Unsupported
    }
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

const fn map_win32_error(error: WIN32_ERROR) -> HardwareError {
    match error {
        ERROR_NOT_ENOUGH_MEMORY | ERROR_INSUFFICIENT_BUFFER => HardwareError::resource_exhausted(),
        ERROR_INVALID_PARAMETER => HardwareError::invalid(),
        ERROR_BUSY => HardwareError::busy(),
        ERROR_NOT_SUPPORTED => HardwareError::unsupported(),
        _ => HardwareError::platform(error.0 as i32),
    }
}

#[cfg(all(test, feature = "std", target_os = "windows"))]
mod tests {
    extern crate std;

    use super::*;

    #[test]
    fn windows_hardware_support_reports_truthful_surface() {
        let support = system_hardware().support();
        assert_eq!(
            support.topology.implementation,
            HardwareImplementationKind::Native
        );

        match cpu_truth_mode() {
            CpuTruthMode::Native => {
                assert_eq!(
                    support.cpu.implementation,
                    HardwareImplementationKind::Native
                );
                assert_eq!(support.cpu.descriptor, HardwareGuarantee::Verified);
                assert_eq!(support.cpu.memory_ordering, HardwareGuarantee::Verified);
                assert_eq!(support.cpu.atomic_widths, HardwareGuarantee::Verified);
                assert_eq!(support.cpu.stack_abi, HardwareGuarantee::Verified);
            }
            CpuTruthMode::Emulated => {
                assert_eq!(
                    support.cpu.implementation,
                    HardwareImplementationKind::Emulated
                );
                assert!(support.cpu.caps.is_empty());
            }
            CpuTruthMode::Unknown => {
                assert_eq!(
                    support.cpu.implementation,
                    HardwareImplementationKind::Unsupported
                );
            }
        }
    }

    #[test]
    fn windows_hardware_topology_summary_and_writes_are_consistent_when_available() {
        let hardware = system_hardware();
        let summary = hardware.topology_summary();

        let Ok(summary) = summary else {
            return;
        };

        if let Some(total) = summary.logical_cpu_count {
            let mut ids = std::vec![
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
            let mut cores = std::vec![ThreadCoreId(0); total];
            let written = hardware
                .write_cores(&mut cores)
                .expect("core write should succeed");
            assert_eq!(written.total, total);
            assert_eq!(written.written, total);
        }

        if let Some(total) = summary.numa_node_count {
            let mut nodes = std::vec![crate::contract::pal::mem::MemTopologyNodeId(0); total];
            let written = hardware
                .write_numa_nodes(&mut nodes)
                .expect("numa node write should succeed");
            assert_eq!(written.total, total);
            assert_eq!(written.written, total);
        }
    }
}
