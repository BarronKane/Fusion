//! FADT definitions and a deliberately constrained fixed-feature parser.
//!
//! The Fixed ACPI Description Table (`FADT`, signature `FACP`) is ACPI's
//! bridge between table discovery and the rest of the fixed hardware model. In
//! the UEFI Forum ACPI Specification 6.6, Section 5.2.9 defines it as the
//! table that:
//!
//! - describes the fixed ACPI hardware register model,
//! - points OSPM at the FACS,
//! - points OSPM at the DSDT, which begins the AML namespace.
//!
//! Fusion is not trying to ingest the entire fixed-register model yet. The
//! first honest bring-up job here is narrower:
//!
//! - validate that a table is one real `FADT`,
//! - surface the raw DSDT and FACS pointers,
//! - prefer the `X_*` 64-bit pointers over their 32-bit legacy counterparts
//!   when the 64-bit field is present and non-zero, exactly as ACPI 6.6
//!   Section 5.2.9 requires,
//! - expose a few fixed fields that are useful for early topology and power
//!   policy decisions.
//!
//! The full FADT is much fatter than this module. That is fine. A minimal,
//! truthful parser now beats a giant speculative one that grows opinions about
//! ACPI hardware programming before the rest of Fusion is ready to consume it.

use bitflags::bitflags;

use super::{AcpiError, AcpiSignature, AcpiTableView, read_unaligned_copy};

const OFFSET_FIRMWARE_CTRL: usize = 36;
const OFFSET_DSDT: usize = 40;
const OFFSET_RESERVED_INT_MODEL: usize = 44;
const OFFSET_PREFERRED_PM_PROFILE: usize = 45;
const OFFSET_SCI_INT: usize = 46;
const OFFSET_FLAGS: usize = 112;
const OFFSET_RESET_VALUE: usize = 128;
const OFFSET_ARM_BOOT_ARCH: usize = 129;
const OFFSET_MINOR_VERSION: usize = 131;
const OFFSET_X_FIRMWARE_CTRL: usize = 132;
const OFFSET_X_DSDT: usize = 140;

bitflags! {
    /// Selected FADT fixed-feature flags.
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub struct FadtFlags: u32 {
        /// WBINVD instruction works properly.
        const WBINVD = 1 << 0;
        /// WBINVD flushes but does not invalidate caches.
        const WBINVD_FLUSH = 1 << 1;
        /// Processor supports `C1` power state.
        const PROC_C1 = 1 << 2;
        /// Sleep button is present.
        const SLP_BUTTON = 1 << 5;
        /// RTC wake status is not valid after waking from S4.
        const FIX_RTC = 1 << 6;
        /// RTC alarm can wake the system from S4.
        const RTC_S4 = 1 << 7;
        /// Reset register/value pair is supported.
        const RESET_REG_SUP = 1 << 10;
        /// System uses a sealed case.
        const SEALED_CASE = 1 << 11;
        /// Headless system.
        const HEADLESS = 1 << 12;
        /// Platform implements reduced ACPI hardware.
        const HW_REDUCED_ACPI = 1 << 20;
        /// Platform supports low-power S0 idle.
        const LOW_POWER_S0_IDLE_CAPABLE = 1 << 21;
    }
}

/// OEM-preferred power-management profile from the FADT.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FadtPreferredPmProfile {
    Unspecified,
    Desktop,
    Mobile,
    Workstation,
    EnterpriseServer,
    SohoServer,
    AppliancePc,
    PerformanceServer,
    Tablet,
    Reserved(u8),
}

impl FadtPreferredPmProfile {
    #[must_use]
    pub const fn from_raw(raw: u8) -> Self {
        match raw {
            0 => Self::Unspecified,
            1 => Self::Desktop,
            2 => Self::Mobile,
            3 => Self::Workstation,
            4 => Self::EnterpriseServer,
            5 => Self::SohoServer,
            6 => Self::AppliancePc,
            7 => Self::PerformanceServer,
            8 => Self::Tablet,
            other => Self::Reserved(other),
        }
    }
}

/// Borrowed validated FADT view.
#[derive(Clone, Copy, Debug)]
pub struct Fadt<'a> {
    table: AcpiTableView<'a>,
    firmware_ctrl: u32,
    dsdt: u32,
    preferred_pm_profile: FadtPreferredPmProfile,
    sci_int: u16,
    flags: Option<FadtFlags>,
    reset_value: Option<u8>,
    arm_boot_arch: Option<u16>,
    minor_version: Option<u8>,
    x_firmware_ctrl: Option<u64>,
    x_dsdt: Option<u64>,
}

impl<'a> Fadt<'a> {
    /// Parses one validated FADT.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the table is malformed, truncated, or not one FADT.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, AcpiError> {
        let table = AcpiTableView::parse_signature(bytes, AcpiSignature::FADT)?;
        let table_bytes = table.bytes();

        let firmware_ctrl = u32::from_le(read_field::<u32>(table_bytes, OFFSET_FIRMWARE_CTRL)?);
        let dsdt = u32::from_le(read_field::<u32>(table_bytes, OFFSET_DSDT)?);
        let reserved_int_model = *table_bytes
            .get(OFFSET_RESERVED_INT_MODEL)
            .ok_or_else(AcpiError::truncated)?;
        if reserved_int_model != 0 && reserved_int_model != 1 {
            return Err(AcpiError::invalid_layout());
        }

        let preferred_pm_profile = FadtPreferredPmProfile::from_raw(
            *table_bytes
                .get(OFFSET_PREFERRED_PM_PROFILE)
                .ok_or_else(AcpiError::truncated)?,
        );
        let sci_int = u16::from_le(read_field::<u16>(table_bytes, OFFSET_SCI_INT)?);
        let flags = read_optional_field::<u32>(table_bytes, OFFSET_FLAGS)
            .map(u32::from_le)
            .map(FadtFlags::from_bits_retain);
        let reset_value = table_bytes.get(OFFSET_RESET_VALUE).copied();
        let arm_boot_arch =
            read_optional_field::<u16>(table_bytes, OFFSET_ARM_BOOT_ARCH).map(u16::from_le);
        let minor_version = table_bytes.get(OFFSET_MINOR_VERSION).copied();
        let x_firmware_ctrl =
            read_optional_field::<u64>(table_bytes, OFFSET_X_FIRMWARE_CTRL).map(u64::from_le);
        let x_dsdt = read_optional_field::<u64>(table_bytes, OFFSET_X_DSDT).map(u64::from_le);

        Ok(Self {
            table,
            firmware_ctrl,
            dsdt,
            preferred_pm_profile,
            sci_int,
            flags,
            reset_value,
            arm_boot_arch,
            minor_version,
            x_firmware_ctrl,
            x_dsdt,
        })
    }

    /// Returns the underlying validated ACPI table view.
    #[must_use]
    pub const fn table(self) -> AcpiTableView<'a> {
        self.table
    }

    /// Returns the 32-bit legacy FACS address.
    #[must_use]
    pub const fn firmware_ctrl(self) -> u32 {
        self.firmware_ctrl
    }

    /// Returns the 32-bit legacy DSDT address.
    #[must_use]
    pub const fn dsdt(self) -> u32 {
        self.dsdt
    }

    /// Returns the preferred power-management profile.
    #[must_use]
    pub const fn preferred_pm_profile(self) -> FadtPreferredPmProfile {
        self.preferred_pm_profile
    }

    /// Returns the SCI interrupt value from the FADT.
    #[must_use]
    pub const fn sci_int(self) -> u16 {
        self.sci_int
    }

    /// Returns the decoded fixed-feature flags when the field exists in this revision.
    #[must_use]
    pub const fn flags(self) -> Option<FadtFlags> {
        self.flags
    }

    /// Returns the raw reset value when present in this revision.
    #[must_use]
    pub const fn reset_value(self) -> Option<u8> {
        self.reset_value
    }

    /// Returns the ARM boot architecture flags when present.
    #[must_use]
    pub const fn arm_boot_arch(self) -> Option<u16> {
        self.arm_boot_arch
    }

    /// Returns the minor FADT version field when present.
    #[must_use]
    pub const fn minor_version(self) -> Option<u8> {
        self.minor_version
    }

    /// Returns the 64-bit FACS pointer when present in this revision.
    #[must_use]
    pub const fn x_firmware_ctrl(self) -> Option<u64> {
        self.x_firmware_ctrl
    }

    /// Returns the 64-bit DSDT pointer when present in this revision.
    #[must_use]
    pub const fn x_dsdt(self) -> Option<u64> {
        self.x_dsdt
    }

    /// Returns the effective FACS address, preferring the `X_FIRMWARE_CTRL` field.
    #[must_use]
    pub fn effective_firmware_ctrl_address(&self) -> u64 {
        match self.x_firmware_ctrl {
            Some(address) if address != 0 => address,
            _ => u64::from(self.firmware_ctrl),
        }
    }

    /// Returns the effective DSDT address, preferring the `X_DSDT` field.
    #[must_use]
    pub fn effective_dsdt_address(&self) -> u64 {
        match self.x_dsdt {
            Some(address) if address != 0 => address,
            _ => u64::from(self.dsdt),
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

fn read_optional_field<T: Copy>(bytes: &[u8], offset: usize) -> Option<T> {
    let end = offset.checked_add(size_of::<T>())?;
    let field = bytes.get(offset..end)?;
    read_unaligned_copy(field).ok()
}

use core::mem::size_of;

#[cfg(test)]
mod tests {
    use super::*;

    fn build_fadt(length: usize) -> Vec<u8> {
        let mut bytes = vec![0_u8; length];
        bytes[0..4].copy_from_slice(b"FACP");
        bytes[4..8].copy_from_slice(&(length as u32).to_le_bytes());
        bytes[8] = 6;
        bytes[10..16].copy_from_slice(b"FUSION");
        bytes[16..24].copy_from_slice(b"FADTTEST");
        bytes[36..40].copy_from_slice(&0x1234_5000_u32.to_le_bytes());
        bytes[40..44].copy_from_slice(&0xDEAD_BEEF_u32.to_le_bytes());
        bytes[44] = 0;
        bytes[45] = 2;
        bytes[46..48].copy_from_slice(&9_u16.to_le_bytes());
        if length >= 116 {
            bytes[112..116].copy_from_slice(
                &(FadtFlags::RESET_REG_SUP | FadtFlags::HW_REDUCED_ACPI)
                    .bits()
                    .to_le_bytes(),
            );
        }
        if length >= 129 {
            bytes[128] = 0xCF;
        }
        if length >= 131 {
            bytes[129..131].copy_from_slice(&0x0001_u16.to_le_bytes());
        }
        if length >= 132 {
            bytes[131] = 6;
        }
        if length >= 140 {
            bytes[132..140].copy_from_slice(&0x0000_0001_2345_6000_u64.to_le_bytes());
        }
        if length >= 148 {
            bytes[140..148].copy_from_slice(&0x0000_0001_F00D_BA5E_u64.to_le_bytes());
        }
        let checksum =
            (!bytes.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte))).wrapping_add(1);
        bytes[9] = checksum;
        bytes
    }

    #[test]
    fn fadt_prefers_x_pointers_when_present() {
        let bytes = build_fadt(148);
        let fadt = Fadt::parse(&bytes).expect("fadt should parse");
        assert_eq!(fadt.firmware_ctrl(), 0x1234_5000);
        assert_eq!(fadt.dsdt(), 0xDEAD_BEEF);
        assert_eq!(
            fadt.effective_firmware_ctrl_address(),
            0x0000_0001_2345_6000
        );
        assert_eq!(fadt.effective_dsdt_address(), 0x0000_0001_F00D_BA5E);
        assert_eq!(fadt.preferred_pm_profile(), FadtPreferredPmProfile::Mobile);
        assert_eq!(fadt.sci_int(), 9);
        assert_eq!(
            fadt.flags(),
            Some(FadtFlags::RESET_REG_SUP | FadtFlags::HW_REDUCED_ACPI)
        );
        assert_eq!(fadt.reset_value(), Some(0xCF));
        assert_eq!(fadt.arm_boot_arch(), Some(1));
        assert_eq!(fadt.minor_version(), Some(6));
    }

    #[test]
    fn fadt_falls_back_to_legacy_pointers_when_x_fields_absent() {
        let bytes = build_fadt(116);
        let fadt = Fadt::parse(&bytes).expect("legacy-sized fadt should parse");
        assert_eq!(fadt.effective_firmware_ctrl_address(), 0x1234_5000);
        assert_eq!(fadt.effective_dsdt_address(), 0xDEAD_BEEF);
        assert_eq!(fadt.x_firmware_ctrl(), None);
        assert_eq!(fadt.x_dsdt(), None);
        assert_eq!(fadt.minor_version(), None);
    }
}
