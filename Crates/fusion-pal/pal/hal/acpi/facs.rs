//! FACS definitions for the firmware-owned ACPI control structure.
//!
//! The Firmware ACPI Control Structure (`FACS`) is not a normal System
//! Description Table. ACPI 6.6 Section 5.2.10 defines it as a reserved
//! read/write firmware structure passed through the FADT, not as one more SDT
//! hanging off the XSDT.
//!
//! That difference matters:
//!
//! - there is no standard SDT `DESCRIPTION_HEADER`,
//! - there is no ACPI checksum field,
//! - the structure lives in reserved read/write memory,
//! - firmware and OSPM both mutate some of its fields at runtime.
//!
//! For Fusion's current scope, this module keeps to the fixed structural truth:
//!
//! - validate the `FACS` signature,
//! - validate the minimum length and reserved-zero fields,
//! - expose the wake-vector fields, global lock word, and fixed flags,
//! - prefer the 64-bit waking vector when it is non-zero, mirroring the spec.
//!
//! The actual Global Lock acquisition protocol from ACPI 6.6 Section 5.2.10.1
//! is intentionally not implemented here yet. That is synchronization policy,
//! not table parsing, and this repo has enough active sins already.

use bitflags::bitflags;

use super::{
    AcpiError,
    AcpiSignature,
    read_unaligned_copy,
};

const MIN_FACS_LENGTH: usize = 64;
const OFFSET_SIGNATURE: usize = 0;
const OFFSET_LENGTH: usize = 4;
const OFFSET_HARDWARE_SIGNATURE: usize = 8;
const OFFSET_FIRMWARE_WAKING_VECTOR: usize = 12;
const OFFSET_GLOBAL_LOCK: usize = 16;
const OFFSET_FLAGS: usize = 20;
const OFFSET_X_FIRMWARE_WAKING_VECTOR: usize = 24;
const OFFSET_VERSION: usize = 32;
const OFFSET_RESERVED0: usize = 33;
const OFFSET_OSPM_FLAGS: usize = 36;
const OFFSET_RESERVED1: usize = 40;

bitflags! {
    /// Firmware-owned FACS feature flags.
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub struct FacsFlags: u32 {
        /// Platform supports `S4BIOS_REQ`.
        const S4BIOS_F = 1 << 0;
        /// Firmware supports 64-bit wake for `X_Firmware_Waking_Vector`.
        const WAKE_64BIT_SUPPORTED = 1 << 1;
    }
}

bitflags! {
    /// OSPM-owned FACS wake flags.
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub struct FacsOspmFlags: u32 {
        /// OSPM requests a 64-bit wake environment for the X waking vector.
        const WAKE_64BIT = 1 << 0;
    }
}

/// Borrowed validated FACS view.
#[derive(Clone, Copy, Debug)]
pub struct Facs<'a> {
    bytes: &'a [u8],
    hardware_signature: u32,
    firmware_waking_vector: u32,
    global_lock: u32,
    flags: FacsFlags,
    x_firmware_waking_vector: u64,
    version: u8,
    ospm_flags: FacsOspmFlags,
}

impl<'a> Facs<'a> {
    /// Parses one validated FACS structure.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the structure is truncated, malformed, or not one FACS.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, AcpiError> {
        let signature = read_field::<[u8; 4]>(bytes, OFFSET_SIGNATURE)?;
        if AcpiSignature::new(signature) != AcpiSignature::FACS {
            return Err(AcpiError::invalid_signature());
        }

        let length = usize::try_from(u32::from_le(read_field::<u32>(bytes, OFFSET_LENGTH)?))
            .map_err(|_| AcpiError::invalid_layout())?;
        if length < MIN_FACS_LENGTH {
            return Err(AcpiError::invalid_layout());
        }
        let facs_bytes = bytes.get(..length).ok_or_else(AcpiError::truncated)?;

        if facs_bytes[OFFSET_RESERVED0..OFFSET_RESERVED0 + 3] != [0, 0, 0] {
            return Err(AcpiError::invalid_layout());
        }
        if facs_bytes[OFFSET_RESERVED1..OFFSET_RESERVED1 + 24]
            .iter()
            .any(|byte| *byte != 0)
        {
            return Err(AcpiError::invalid_layout());
        }

        let raw_flags = u32::from_le(read_field::<u32>(facs_bytes, OFFSET_FLAGS)?);
        if raw_flags & !FacsFlags::all().bits() != 0 {
            return Err(AcpiError::invalid_layout());
        }
        let raw_ospm_flags = u32::from_le(read_field::<u32>(facs_bytes, OFFSET_OSPM_FLAGS)?);
        if raw_ospm_flags & !FacsOspmFlags::all().bits() != 0 {
            return Err(AcpiError::invalid_layout());
        }

        Ok(Self {
            bytes: facs_bytes,
            hardware_signature: u32::from_le(read_field::<u32>(
                facs_bytes,
                OFFSET_HARDWARE_SIGNATURE,
            )?),
            firmware_waking_vector: u32::from_le(read_field::<u32>(
                facs_bytes,
                OFFSET_FIRMWARE_WAKING_VECTOR,
            )?),
            global_lock: u32::from_le(read_field::<u32>(facs_bytes, OFFSET_GLOBAL_LOCK)?),
            flags: FacsFlags::from_bits_retain(raw_flags),
            x_firmware_waking_vector: u64::from_le(read_field::<u64>(
                facs_bytes,
                OFFSET_X_FIRMWARE_WAKING_VECTOR,
            )?),
            version: *facs_bytes
                .get(OFFSET_VERSION)
                .ok_or_else(AcpiError::truncated)?,
            ospm_flags: FacsOspmFlags::from_bits_retain(raw_ospm_flags),
        })
    }

    /// Returns the validated raw FACS bytes.
    #[must_use]
    pub const fn bytes(self) -> &'a [u8] {
        self.bytes
    }

    /// Returns the total FACS length.
    #[must_use]
    pub fn length(&self) -> usize {
        self.bytes.len()
    }

    /// Returns the ACPI hardware signature for the current boot.
    #[must_use]
    pub const fn hardware_signature(self) -> u32 {
        self.hardware_signature
    }

    /// Returns the 32-bit firmware waking vector.
    #[must_use]
    pub const fn firmware_waking_vector(self) -> u32 {
        self.firmware_waking_vector
    }

    /// Returns the raw global-lock dword.
    #[must_use]
    pub const fn global_lock(self) -> u32 {
        self.global_lock
    }

    /// Returns the firmware-owned FACS flags.
    #[must_use]
    pub const fn flags(self) -> FacsFlags {
        self.flags
    }

    /// Returns the 64-bit firmware waking vector.
    #[must_use]
    pub const fn x_firmware_waking_vector(self) -> u64 {
        self.x_firmware_waking_vector
    }

    /// Returns the FACS version byte.
    #[must_use]
    pub const fn version(self) -> u8 {
        self.version
    }

    /// Returns the OSPM-controlled FACS flags.
    #[must_use]
    pub const fn ospm_flags(self) -> FacsOspmFlags {
        self.ospm_flags
    }

    /// Returns the effective waking vector, preferring the 64-bit field.
    #[must_use]
    pub fn effective_firmware_waking_vector(&self) -> u64 {
        if self.x_firmware_waking_vector != 0 {
            self.x_firmware_waking_vector
        } else {
            u64::from(self.firmware_waking_vector)
        }
    }
}

fn read_field<T: Copy>(bytes: &[u8], offset: usize) -> Result<T, AcpiError> {
    let end = offset
        .checked_add(size_of::<T>())
        .ok_or_else(AcpiError::invalid_layout)?;
    let field = bytes.get(offset..end).ok_or_else(AcpiError::truncated)?;
    read_unaligned_copy(field)
}

use core::mem::size_of;

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    fn build_facs() -> Vec<u8> {
        let mut bytes = vec![0_u8; 64];
        bytes[0..4].copy_from_slice(b"FACS");
        bytes[4..8].copy_from_slice(&(64_u32).to_le_bytes());
        bytes[8..12].copy_from_slice(&0xAABB_CCDD_u32.to_le_bytes());
        bytes[12..16].copy_from_slice(&0x0008_0000_u32.to_le_bytes());
        bytes[16..20].copy_from_slice(&0x0000_0002_u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&FacsFlags::WAKE_64BIT_SUPPORTED.bits().to_le_bytes());
        bytes[24..32].copy_from_slice(&0x0000_0001_0008_0000_u64.to_le_bytes());
        bytes[32] = 3;
        bytes[36..40].copy_from_slice(&FacsOspmFlags::WAKE_64BIT.bits().to_le_bytes());
        bytes
    }

    #[test]
    fn facs_prefers_x_waking_vector() {
        let bytes = build_facs();
        let facs = Facs::parse(&bytes).expect("facs should parse");
        assert_eq!(facs.length(), 64);
        assert_eq!(facs.hardware_signature(), 0xAABB_CCDD);
        assert_eq!(facs.firmware_waking_vector(), 0x0008_0000);
        assert_eq!(facs.global_lock(), 0x0000_0002);
        assert_eq!(facs.flags(), FacsFlags::WAKE_64BIT_SUPPORTED);
        assert_eq!(facs.version(), 3);
        assert_eq!(facs.ospm_flags(), FacsOspmFlags::WAKE_64BIT);
        assert_eq!(
            facs.effective_firmware_waking_vector(),
            0x0000_0001_0008_0000
        );
    }

    #[test]
    fn facs_rejects_non_zero_reserved_bytes() {
        let mut bytes = build_facs();
        bytes[40] = 1;
        let error = Facs::parse(&bytes).expect_err("reserved bytes must stay zero");
        assert_eq!(error.kind(), super::super::AcpiErrorKind::InvalidLayout);
    }
}
