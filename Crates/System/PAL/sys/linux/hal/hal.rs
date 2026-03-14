//! Linux PAL hardware backend.

use core::str;

use rustix::fs::{CWD, Mode, OFlags, openat};
use rustix::io::{Errno, read};
use rustix::thread::{self as rustix_thread, CpuSet};

use crate::pal::hal::{
    HardwareAuthoritySet, HardwareBase, HardwareCpuCaps, HardwareCpuDescription, HardwareCpuQuery,
    HardwareCpuSupport, HardwareCpuVendor, HardwareError, HardwareErrorKind, HardwareGuarantee,
    HardwareImplementationKind, HardwareSimdSet, HardwareStackAbi, HardwareSupport,
    HardwareTopologyCaps, HardwareTopologyNodeId, HardwareTopologyQuery, HardwareTopologySummary,
    HardwareTopologySupport, HardwareWriteSummary,
};
use crate::pal::thread::{ThreadLogicalCpuId, ThreadProcessorGroupId};

/// Selected Linux hardware provider type.
#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxHardware;

/// Target-selected Linux hardware provider alias.
pub type PlatformHardware = LinuxHardware;

impl LinuxHardware {
    /// Creates a new Linux hardware provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

/// Returns the selected Linux hardware provider.
#[must_use]
pub const fn system_hardware() -> PlatformHardware {
    PlatformHardware::new()
}

const NODE_ONLINE_PATH: &str = "/sys/devices/system/node/online";
const CACHE_PATH_PREFIX: &[u8] = b"/sys/devices/system/cpu/cpu";
const CACHE_PATH_MIDDLE: &[u8] = b"/cache/index";
const CACHE_PATH_SUFFIX: &[u8] = b"/coherency_line_size";

impl HardwareBase for LinuxHardware {
    fn support(&self) -> HardwareSupport {
        support()
    }
}

impl HardwareCpuQuery for LinuxHardware {
    fn cpu_description(&self) -> Result<HardwareCpuDescription, HardwareError> {
        Ok(cpu_description())
    }

    fn stack_abi(&self) -> Result<HardwareStackAbi, HardwareError> {
        crate::hal::system_hardware().stack_abi()
    }
}

impl HardwareTopologyQuery for LinuxHardware {
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
        _output: &mut [crate::pal::thread::ThreadCoreId],
    ) -> Result<HardwareWriteSummary, HardwareError> {
        Err(HardwareError::unsupported())
    }

    fn write_clusters(
        &self,
        _output: &mut [crate::pal::thread::ThreadClusterId],
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
        _output: &mut [crate::pal::thread::ThreadCoreClassId],
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
            stack_abi: crate::hal::stack_abi_guarantee(),
            simd,
            authorities: cpu_authorities(cache_line_bytes, simd),
            implementation: HardwareImplementationKind::Native,
        },
        topology,
    }
}

fn cpu_description() -> HardwareCpuDescription {
    HardwareCpuDescription {
        architecture: crate::hal::selected_architecture(),
        vendor: runtime_vendor(),
        endianness: crate::hal::selected_endianness(),
        cache_line_bytes: runtime_cache_line_bytes().or(crate::hal::selected_cache_line_bytes()),
        memory_ordering: crate::hal::selected_memory_ordering(),
        pointer_width_bits: crate::hal::selected_pointer_width_bits(),
        atomic_widths: crate::hal::selected_atomic_widths(),
        simd: runtime_simd_set(),
    }
}

fn topology_summary() -> Result<HardwareTopologySummary, HardwareError> {
    let logical_cpu_count = visible_cpuset().map(|cpuset| cpuset.count() as usize).ok();
    let numa_node_count = online_node_count().ok();

    if logical_cpu_count.is_none() && numa_node_count.is_none() {
        return Err(HardwareError::unsupported());
    }

    Ok(HardwareTopologySummary {
        logical_cpu_count,
        core_count: None,
        cluster_count: None,
        package_count: None,
        numa_node_count,
        core_class_count: None,
    })
}

fn write_logical_cpus(
    output: &mut [ThreadLogicalCpuId],
) -> Result<HardwareWriteSummary, HardwareError> {
    let cpuset = visible_cpuset()?;
    let mut total = 0;
    let mut written = 0;

    for cpu in 0..CpuSet::MAX_CPU {
        if !cpuset.is_set(cpu) {
            continue;
        }

        let index = u16::try_from(cpu).map_err(|_| HardwareError::platform(libc::EOVERFLOW))?;
        if written < output.len() {
            output[written] = ThreadLogicalCpuId {
                group: ThreadProcessorGroupId(0),
                index,
            };
            written += 1;
        }
        total += 1;
    }

    Ok(HardwareWriteSummary::new(total, written))
}

fn write_numa_nodes(
    output: &mut [HardwareTopologyNodeId],
) -> Result<HardwareWriteSummary, HardwareError> {
    let mut buf = [0u8; 128];
    let bytes = read_text_file(NODE_ONLINE_PATH, &mut buf)?;
    let mut written = 0;

    let total = parse_index_list(bytes, |index| {
        if written < output.len() {
            output[written] = crate::pal::mem::MemTopologyNodeId(index);
            written += 1;
        }
        Ok(())
    })?;

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
    let logical_cpus = visible_cpuset().map(|_| ()).is_ok();
    let numa_nodes = online_node_count().is_ok();

    let mut caps = HardwareTopologyCaps::empty();
    let mut summary = HardwareGuarantee::Unsupported;
    let mut logical_cpus_guarantee = HardwareGuarantee::Unsupported;
    let mut numa_nodes_guarantee = HardwareGuarantee::Unsupported;

    if logical_cpus {
        caps |= HardwareTopologyCaps::SUMMARY | HardwareTopologyCaps::LOGICAL_CPUS;
        summary = HardwareGuarantee::Verified;
        logical_cpus_guarantee = HardwareGuarantee::Verified;
    }

    if numa_nodes {
        caps |= HardwareTopologyCaps::SUMMARY | HardwareTopologyCaps::NUMA_NODES;
        summary = HardwareGuarantee::Verified;
        numa_nodes_guarantee = HardwareGuarantee::Verified;
    }

    if caps.is_empty() {
        return HardwareTopologySupport::unsupported();
    }

    HardwareTopologySupport {
        caps,
        summary,
        logical_cpus: logical_cpus_guarantee,
        cores: HardwareGuarantee::Unsupported,
        clusters: HardwareGuarantee::Unsupported,
        packages: HardwareGuarantee::Unsupported,
        numa_nodes: numa_nodes_guarantee,
        core_classes: HardwareGuarantee::Unsupported,
        authorities: HardwareAuthoritySet::OPERATING_SYSTEM | HardwareAuthoritySet::TOPOLOGY,
        implementation: HardwareImplementationKind::Native,
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
const fn runtime_vendor_guarantee() -> HardwareGuarantee {
    if vendor_is_runtime_known() {
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
    if runtime_cache_line_bytes().is_some() || crate::hal::selected_cache_line_bytes().is_some() {
        HardwareGuarantee::Verified
    } else {
        HardwareGuarantee::Unsupported
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
const fn runtime_simd_guarantee() -> HardwareGuarantee {
    if simd_is_runtime_known() {
        HardwareGuarantee::Verified
    } else {
        HardwareGuarantee::Unsupported
    }
}

#[cfg(any(target_arch = "aarch64", target_arch = "arm"))]
fn runtime_simd_guarantee() -> HardwareGuarantee {
    if simd_is_runtime_known() {
        HardwareGuarantee::Verified
    } else {
        HardwareGuarantee::Unsupported
    }
}

#[cfg(not(any(
    target_arch = "x86",
    target_arch = "x86_64",
    target_arch = "aarch64",
    target_arch = "arm"
)))]
const fn runtime_simd_guarantee() -> HardwareGuarantee {
    HardwareGuarantee::Unsupported
}

fn visible_cpuset() -> Result<CpuSet, HardwareError> {
    rustix_thread::sched_getaffinity(None).map_err(map_errno)
}

fn online_node_count() -> Result<usize, HardwareError> {
    let mut buf = [0u8; 128];
    let bytes = read_text_file(NODE_ONLINE_PATH, &mut buf)?;
    parse_index_list(bytes, |_| Ok(()))
}

fn runtime_cache_line_bytes() -> Option<usize> {
    runtime_cache_line_bytes_from_sysfs().or_else(runtime_cache_line_bytes_from_cpu)
}

fn runtime_cache_line_bytes_from_sysfs() -> Option<usize> {
    let cpu = first_visible_cpu()?;
    let mut path_buf = [0u8; 128];
    let mut value_buf = [0u8; 64];

    for index in 0..8 {
        let path = build_cache_line_path(cpu, index, &mut path_buf).ok()?;
        let bytes = match read_text_file(path, &mut value_buf) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == HardwareErrorKind::Unsupported => continue,
            Err(_) => continue,
        };

        if let Ok(value) = parse_decimal_usize(bytes)
            && value > 0
        {
            return Some(value);
        }
    }

    None
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn runtime_cache_line_bytes_from_cpu() -> Option<usize> {
    #[cfg(target_arch = "x86")]
    use core::arch::x86::__cpuid;
    #[cfg(target_arch = "x86_64")]
    use core::arch::x86_64::__cpuid;

    let leaf1 = __cpuid(1);
    if (leaf1.edx & (1 << 19)) == 0 {
        return None;
    }

    let line_size = ((leaf1.ebx >> 8) & 0xff) * 8;
    usize::try_from(line_size).ok().filter(|value| *value > 0)
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
fn runtime_cache_line_bytes_from_cpu() -> Option<usize> {
    None
}

fn first_visible_cpu() -> Option<u32> {
    let cpuset = visible_cpuset().ok()?;
    for cpu in 0..CpuSet::MAX_CPU {
        if cpuset.is_set(cpu) {
            return u32::try_from(cpu).ok();
        }
    }
    None
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn runtime_vendor() -> HardwareCpuVendor {
    #[cfg(target_arch = "x86")]
    use core::arch::x86::__cpuid;
    #[cfg(target_arch = "x86_64")]
    use core::arch::x86_64::__cpuid;

    let leaf0 = __cpuid(0);
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

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
fn runtime_vendor() -> HardwareCpuVendor {
    HardwareCpuVendor::Unknown
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn runtime_simd_set() -> HardwareSimdSet {
    #[cfg(target_arch = "x86")]
    use core::arch::x86::{__cpuid, __cpuid_count, _xgetbv};
    #[cfg(target_arch = "x86_64")]
    use core::arch::x86_64::{__cpuid, __cpuid_count, _xgetbv};

    let leaf0 = __cpuid(0);
    if leaf0.eax < 1 {
        return HardwareSimdSet::empty();
    }

    let leaf1 = __cpuid(1);
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
        let leaf7 = __cpuid_count(7, 0);
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
fn runtime_simd_set() -> HardwareSimdSet {
    let mut simd = HardwareSimdSet::empty();
    if let Some((hwcap, hwcap2)) = runtime_linux_hwcap() {
        if (hwcap & (1 << 1)) != 0 {
            simd |= HardwareSimdSet::NEON;
        }
        if (hwcap & (1 << 22)) != 0 {
            simd |= HardwareSimdSet::SVE;
        }
        if (hwcap2 & (1 << 1)) != 0 {
            simd |= HardwareSimdSet::SVE2;
        }
    }
    simd
}

#[cfg(target_arch = "arm")]
fn runtime_simd_set() -> HardwareSimdSet {
    let mut simd = HardwareSimdSet::empty();
    if let Some((hwcap, _)) = runtime_linux_hwcap()
        && (hwcap & (1 << 12)) != 0
    {
        simd |= HardwareSimdSet::NEON;
    }
    simd
}

#[cfg(not(any(
    target_arch = "x86",
    target_arch = "x86_64",
    target_arch = "aarch64",
    target_arch = "arm"
)))]
fn runtime_simd_set() -> HardwareSimdSet {
    HardwareSimdSet::empty()
}

const fn bit32(value: u32, bit: u32) -> bool {
    (value & (1u32 << bit)) != 0
}

const fn map_errno(errno: Errno) -> HardwareError {
    match errno {
        Errno::NOMEM => HardwareError::resource_exhausted(),
        Errno::INVAL => HardwareError::invalid(),
        Errno::BUSY => HardwareError::busy(),
        Errno::NOENT | Errno::NOTDIR => HardwareError::unsupported(),
        _ => HardwareError::platform(errno.raw_os_error()),
    }
}

fn read_text_file<'a>(path: &str, buf: &'a mut [u8]) -> Result<&'a [u8], HardwareError> {
    let fd = openat(CWD, path, OFlags::RDONLY, Mode::empty()).map_err(map_errno)?;
    let nread = read(&fd, &mut *buf).map_err(map_errno)?;
    if nread == buf.len() {
        return Err(HardwareError::resource_exhausted());
    }
    Ok(trim_ascii_whitespace(&buf[..nread]))
}

fn build_cache_line_path(cpu: u32, index: u32, output: &mut [u8]) -> Result<&str, HardwareError> {
    let mut written = 0;
    written = write_bytes(output, written, CACHE_PATH_PREFIX)?;
    written = write_decimal_u32(output, written, cpu)?;
    written = write_bytes(output, written, CACHE_PATH_MIDDLE)?;
    written = write_decimal_u32(output, written, index)?;
    written = write_bytes(output, written, CACHE_PATH_SUFFIX)?;
    str::from_utf8(&output[..written]).map_err(|_| HardwareError::invalid())
}

fn write_bytes(output: &mut [u8], offset: usize, bytes: &[u8]) -> Result<usize, HardwareError> {
    let end = offset
        .checked_add(bytes.len())
        .ok_or_else(HardwareError::resource_exhausted)?;
    if end > output.len() {
        return Err(HardwareError::resource_exhausted());
    }
    output[offset..end].copy_from_slice(bytes);
    Ok(end)
}

fn write_decimal_u32(output: &mut [u8], offset: usize, value: u32) -> Result<usize, HardwareError> {
    let mut digits = [0u8; 10];
    let mut len = 0;
    let mut current = value;

    loop {
        digits[len] = b'0' + (current % 10) as u8;
        len += 1;
        current /= 10;
        if current == 0 {
            break;
        }
    }

    let end = offset
        .checked_add(len)
        .ok_or_else(HardwareError::resource_exhausted)?;
    if end > output.len() {
        return Err(HardwareError::resource_exhausted());
    }

    for (index, digit) in digits[..len].iter().rev().enumerate() {
        output[offset + index] = *digit;
    }

    Ok(end)
}

fn trim_ascii_whitespace(bytes: &[u8]) -> &[u8] {
    let mut start = 0;
    let mut end = bytes.len();

    while start < end && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    while end > start && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }

    &bytes[start..end]
}

fn parse_decimal_u32(bytes: &[u8]) -> Result<u32, HardwareError> {
    let bytes = trim_ascii_whitespace(bytes);
    if bytes.is_empty() {
        return Err(HardwareError::invalid());
    }

    let mut value = 0u32;
    for byte in bytes {
        if !byte.is_ascii_digit() {
            return Err(HardwareError::invalid());
        }
        value = value
            .checked_mul(10)
            .and_then(|current| current.checked_add(u32::from(byte - b'0')))
            .ok_or_else(HardwareError::invalid)?;
    }

    Ok(value)
}

fn parse_decimal_usize(bytes: &[u8]) -> Result<usize, HardwareError> {
    let value = parse_decimal_u32(bytes)?;
    usize::try_from(value).map_err(|_| HardwareError::platform(libc::EOVERFLOW))
}

fn parse_index_list(
    bytes: &[u8],
    mut on_index: impl FnMut(u32) -> Result<(), HardwareError>,
) -> Result<usize, HardwareError> {
    let mut cursor = 0;
    let bytes = trim_ascii_whitespace(bytes);
    let mut total = 0usize;

    while cursor < bytes.len() {
        while cursor < bytes.len() && (bytes[cursor] == b',' || bytes[cursor].is_ascii_whitespace())
        {
            cursor += 1;
        }
        if cursor >= bytes.len() {
            break;
        }

        let start = parse_list_value(bytes, &mut cursor)?;
        let end = if cursor < bytes.len() && bytes[cursor] == b'-' {
            cursor += 1;
            parse_list_value(bytes, &mut cursor)?
        } else {
            start
        };

        if end < start {
            return Err(HardwareError::invalid());
        }

        for index in start..=end {
            on_index(index)?;
            total += 1;
        }
    }

    Ok(total)
}

fn parse_list_value(bytes: &[u8], cursor: &mut usize) -> Result<u32, HardwareError> {
    let start = *cursor;
    while *cursor < bytes.len() && bytes[*cursor].is_ascii_digit() {
        *cursor += 1;
    }
    parse_decimal_u32(&bytes[start..*cursor])
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
const fn vendor_is_runtime_known() -> bool {
    true
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
const fn vendor_is_runtime_known() -> bool {
    false
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
const fn simd_is_runtime_known() -> bool {
    true
}

#[cfg(any(target_arch = "aarch64", target_arch = "arm"))]
fn simd_is_runtime_known() -> bool {
    runtime_linux_hwcap().is_some()
}

#[cfg(not(any(
    target_arch = "x86",
    target_arch = "x86_64",
    target_arch = "aarch64",
    target_arch = "arm"
)))]
const fn simd_is_runtime_known() -> bool {
    false
}

#[cfg(any(target_arch = "aarch64", target_arch = "arm"))]
fn runtime_linux_hwcap() -> Option<(usize, usize)> {
    let hwcap = unsafe { libc::getauxval(libc::AT_HWCAP) as usize };
    let hwcap2 = unsafe { libc::getauxval(libc::AT_HWCAP2) as usize };
    if hwcap == 0 && hwcap2 == 0 {
        None
    } else {
        Some((hwcap, hwcap2))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_index_list_counts_ranges_and_singles() {
        let mut values = [0u32; 8];
        let mut written = 0usize;
        let total = parse_index_list(b"0-3,8,10-11", |index| {
            values[written] = index;
            written += 1;
            Ok(())
        })
        .expect("range list should parse");

        assert_eq!(total, 7);
        assert_eq!(&values[..written], &[0, 1, 2, 3, 8, 10, 11]);
    }
}
