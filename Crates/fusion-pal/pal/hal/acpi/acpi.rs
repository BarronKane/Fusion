//! Minimal ACPI table parsing for the dynamic HAL lane.
//!
//! This module is the start of Fusion's firmware-topology path for generic
//! bare-metal platforms. The immediate goal is narrow and honest: parse enough
//! of the ACPI table set to discover interrupt topology and PCIe configuration
//! windows without pretending we already have a full AML interpreter or a giant
//! ACPI subsystem strapped to the repo.
//!
//! The normative references for this layer are the UEFI Forum ACPI
//! Specification 6.6 sections that define the table envelope and the first
//! discovery hop:
//!
//! - Section 5.2.5.2 for finding the RSDP on UEFI-enabled systems,
//! - Section 5.2.6 for the common `DESCRIPTION_HEADER`,
//! - Section 5.2.8 for `XSDT`,
//! - Section 5.2.9 for `FADT`,
//! - Section 5.2.12 for `MADT`.
//!
//! `MCFG` is the one deliberately awkward exception. ACPI reserves the
//! signature and points OSPM at the table through the XSDT, but the payload
//! layout itself is defined by the PCI Firmware Specification rather than by
//! ACPI alone. That split is real, so Fusion names it instead of sweeping it
//! under the rug like a proper legacy kernel.
//!
//! Everything in this module stays table-first and validation-heavy:
//!
//! - signatures must match,
//! - the declared table length must fit the supplied bytes,
//! - the whole table checksum must validate,
//! - variable-length records must not overrun the table payload.
//!
//! That gives later topology code a small piece of trustworthy firmware truth
//! instead of a sprawling pile of optimistic byte-casting.

mod dsdt;
mod error;
mod fadt;
mod header;
mod madt;
mod mcfg;
mod xsdt;

pub use dsdt::*;
pub use error::*;
pub use fadt::*;
pub use header::*;
pub use madt::*;
pub use mcfg::*;
pub use xsdt::*;

use core::mem::MaybeUninit;
use core::mem::size_of;
use core::ptr;

pub(crate) fn read_unaligned_copy<T: Copy>(bytes: &[u8]) -> Result<T, AcpiError> {
    if bytes.len() < size_of::<T>() {
        return Err(AcpiError::truncated());
    }

    let mut value = MaybeUninit::<T>::uninit();
    // SAFETY: `value` points to one properly allocated `T`, and we copy exactly `size_of::<T>()`
    // bytes from the input slice after verifying that many bytes exist.
    unsafe {
        ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            value.as_mut_ptr().cast::<u8>(),
            size_of::<T>(),
        );
        Ok(value.assume_init())
    }
}

pub(crate) fn checksum_is_valid(bytes: &[u8]) -> bool {
    bytes.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte)) == 0
}
