//! Common Cortex-M core facts derived from architected core registers.

use cortex_m::peripheral::CPUID;

use crate::contract::hal::{HardwareCpuVendor, HardwareStackAbi, HardwareStackDirection};

/// Decoded view of the architected Cortex-M CPUID register.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CortexMCpuid {
    pub raw: u32,
    pub implementer: u8,
    pub variant: u8,
    pub architecture: u8,
    pub part_number: u16,
    pub revision: u8,
    pub patch_major: u8,
    pub patch_minor: u8,
}

impl CortexMCpuid {
    /// Returns the coarse CPU vendor that can be derived from the CPUID implementer field.
    #[must_use]
    pub const fn vendor(self) -> HardwareCpuVendor {
        match self.implementer {
            0x41 => HardwareCpuVendor::Arm,
            0x00 => HardwareCpuVendor::Unknown,
            _ => HardwareCpuVendor::Other,
        }
    }
}

/// Returns the architected Cortex-M stack ABI contract.
#[must_use]
pub const fn stack_abi() -> HardwareStackAbi {
    HardwareStackAbi {
        min_stack_alignment: 8,
        red_zone_bytes: 0,
        direction: HardwareStackDirection::Down,
        guard_required: Some(false),
    }
}

/// Reads the architected Cortex-M CPUID register and returns a decoded view.
#[must_use]
#[inline]
pub fn read_cpuid() -> CortexMCpuid {
    let raw = unsafe { (*CPUID::PTR).base.read() };

    CortexMCpuid {
        raw,
        implementer: ((raw >> 24) & 0xff) as u8,
        variant: ((raw >> 20) & 0x0f) as u8,
        architecture: ((raw >> 16) & 0x0f) as u8,
        part_number: ((raw >> 4) & 0x0fff) as u16,
        revision: (raw & 0x0f) as u8,
        patch_major: ((raw >> 20) & 0x0f) as u8,
        patch_minor: (raw & 0x0f) as u8,
    }
}
