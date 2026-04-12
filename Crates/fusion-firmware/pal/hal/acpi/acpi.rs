//! Minimal ACPI table parsing for the dynamic HAL lane.
//!
//! https://uefi.org/specs/ACPI/6.6/Frontmatter/List_of_Tables.html
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
//! - Section 5.2.8 for `XSDT` (Extended System Description Table),
//! - Section 5.2.9 for `FADT` (Fixed ACPI Description Table),
//! - Section 5.2.12 for `MADT` (Multiple APIC Description Table).
//!
//! `MCFG` is the one deliberately awkward exception. ACPI reserves the
//! signature and points OSPM at the table through the XSDT, but the payload
//! layout itself is defined by the PCI Firmware Specification rather than by
//! ACPI alone. That split is real, so Fusion names it instead of sweeping it
//! under the rug like a proper legacy kernel.
//!
//! Thin map of the ACPI data-structure universe, at least at the level this
//! HAL parser cares about:
//!
//! - bootstrap and discovery:
//!   - `RSDP` (Root System Description Pointer)
//!   - the common `DESCRIPTION_HEADER`
//!   - `RSDT` (Root System Description Table) / `XSDT` (Extended System
//!     Description Table)
//! - fixed firmware and control handoff:
//!   - `FADT` (Fixed ACPI Description Table)
//!   - `FACS` (Firmware ACPI Control Structure)
//!   - Generic Address Structures and the fixed ACPI register model carried by
//!     `FADT`
//! - AML definition blocks and namespace roots:
//!   - `DSDT` (Differentiated System Description Table)
//!   - `SSDT` (Secondary System Description Table)
//!   - `PSDT` (Persistent System Description Table)
//!   - the AML namespace and resource-description objects those blocks define
//! - interrupt and controller topology:
//!   - `MADT` (Multiple APIC Description Table)
//!   - APIC (Advanced Programmable Interrupt Controller) / SAPIC (Streamlined
//!     Advanced Programmable Interrupt Controller) / x2APIC records
//!   - GIC (Generic Interrupt Controller) records
//!   - RISC-V interrupt-controller records
//!   - Global System Interrupt numbering
//! - early platform-service and boot-description tables:
//!   - `SBST` (Smart Battery Specification Table)
//!   - `ECDT` (Embedded Controller Boot Resources Table)
//!   - `BGRT` (Boot Graphics Resource Table)
//!   - `FPDT` (Firmware Performance Data Table)
//!   - `GTDT` (Generic Timer Description Table)
//!   - `NHLT` (Non HD Audio Link Table)
//! - topology, locality, and memory description:
//!   - `SRAT` (System Resource Affinity Table)
//!   - `SLIT` (System Locality Information Table)
//!   - `CPEP` (Corrected Platform Error Polling Table)
//!   - `MSCT` (Maximum System Characteristics Table)
//!   - `MPST` (Memory Power State Table)
//!   - `PMTT` (Platform Memory Topology Table)
//!   - `HMAT` (Heterogeneous Memory Attribute Table)
//!   - `NFIT` (NVDIMM Firmware Interface Table)
//! - RAS, security, health, and debug-adjacent platform tables:
//!   - `RASF` (ACPI RAS Feature Table)
//!   - `RAS2` (ACPI RAS2 Feature Table)
//!   - `SDEV` (Secure Devices Table)
//!   - `PDTT` (Platform Debug Trigger Table)
//!   - `PHAT` (Platform Health Assessment Table)
//! - I/O translation and modern platform description:
//!   - `VIOT` (Virtual I/O Translation Table)
//!   - plus a long tail of architecture-, vendor-, and transport-specific
//!     tables and substructures
//!
//! That is the thin map. The fat map is worse. Much worse. ACPI is not one
//! table format; it is a firmware metadata civilization with a bytecode cult in
//! the middle of it.
//!
//! Everything in this module stays table-first and validation-heavy:
//!
//! - signatures must match,
//! - the declared table length must fit the supplied bytes,
//! - the whole table checksum must validate,
//! - variable-length records must not overrun the table payload.
//!
//! That gives later topology code a small piece of trustworthy firmware truth
//! instead of a sprawling pile of optimistic byte-casting. Right now Fusion is
//! only carving out the early spine:
//!
//! - `XSDT` (Extended System Description Table)
//! - `FADT` (Fixed ACPI Description Table)
//! - `FACS` (Firmware ACPI Control Structure)
//! - `DSDT` (Differentiated System Description Table)
//! - `MCFG` (PCI Express Memory-mapped Configuration Space base address
//!   description table)
//! - `MADT` (Multiple APIC Description Table)
//!
//! Everything else can wait its turn in the standards minefield.

mod dsdt;
mod error;
mod facs;
mod fadt;
mod header;
mod madt;
mod mcfg;
mod realize;
mod xsdt;

use core::mem::MaybeUninit;
use core::mem::size_of;
use core::ptr;

pub use dsdt::*;
pub use error::*;
pub use facs::*;
pub use fadt::*;
pub use header::*;
pub use madt::*;
pub use mcfg::*;
pub use realize::*;
pub use xsdt::*;

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
