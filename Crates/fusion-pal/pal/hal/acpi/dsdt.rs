//! DSDT definitions and a deliberately tiny stub parser.
//!
//! The Differentiated System Description Table (`DSDT`) is where ACPI stops
//! being a neat table-walk problem and starts trying to drag the OS into AML,
//! namespace construction, control methods, and vendor creativity.
//!
//! In the UEFI Forum ACPI Specification 6.6:
//!
//! - Section 5.2.11.1 defines the `DSDT` as a standard SDT carrying a
//!   Definition Block,
//! - the frontmatter overview calls it the beginning of the ACPI namespace,
//! - Chapter 20 defines the AML bytecode living inside that Definition Block.
//!
//! For Fusion's current bring-up scope, this module does the only honest thing:
//!
//! - validate that a table is actually one `DSDT`,
//! - expose the raw AML byte stream as borrowed bytes,
//! - refuse to pretend we already implement namespace loading, AML execution,
//!   or vendor-specific control-method coping strategies.
//!
//! That keeps the HAL layer truthful. The parser can recognize the horror
//! movie without volunteering to become the protagonist.
//!
//! --------------------- WITH THAT ASIDE ---------------------
//!
//! I swear Fusion just started with me wanting to build avionics, which meant
//! I just needed a critical-safe aware memory allocator. I didn't mean to get this deep.
//!
//! But really? What the fuck? What the actual fuck. What is this?
//! I did some looking around, and I feel like I entered a horror show.
//! I peeked into the linux kernel and...
//!
//! Alright, look, this is a proper hellscape:
//!
//! -- Methods that return the wrong type depending on OS version string.
//! -- _OSI("Linux") checks that change behavior.
//! -- -- It doesn't surprise me that vendors deliberately break linux paths.
//! -- Battery status methods that deadlock.
//! -- Thermal zone methods that only work if you call them in a specific order.
//! -- Sleep/wake methods that assume Windows Power Manager behavior.
//! -- -- (Thanks Linus (LTT) for this one, I look forward to getting fusion on
//! -- -- my i5 hp laptop I have lying around from 2010.)
//!
//! Some vendor-specific notes:
//!
//! -- Lenovo... I'm just... moving on.
//! -- HP consumer laptops - Creative interpretations of the specification.
//! -- ASUS - Don't trust them for security. Okay for desktops, iffy for laptops.
//! -- Dell - Server boards (poweredge) are solid. Consumer is mixed.
//! -- Apple - They use DeviceTree, not ACPI, which is in some ways a mercy.
//! -- Supermicro/server boards - Generally the most compliant from enterprise pressure.
//!
//! I could go on, but I suspect DSDT is going to end up being the fattest module
//! in all of Fusion. This is why Linux has 30+ years of organic growth. I'm not going
//! to pretend to ever have the delusion that I could do it all myself, nor would I
//! want to, but for anyone that comes across this that wants to add a vendor...
//!
//! I salute you, and good luck.

use super::{AcpiError, AcpiSignature, AcpiTableView};

/// Borrowed validated DSDT view.
#[derive(Clone, Copy, Debug)]
pub struct Dsdt<'a> {
    table: AcpiTableView<'a>,
}

impl<'a> Dsdt<'a> {
    /// Parses one validated DSDT.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the table is malformed, truncated, or not one DSDT.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, AcpiError> {
        Ok(Self {
            table: AcpiTableView::parse_signature(bytes, AcpiSignature::DSDT)?,
        })
    }

    /// Returns the underlying validated ACPI table view.
    #[must_use]
    pub const fn table(self) -> AcpiTableView<'a> {
        self.table
    }

    /// Returns the raw AML Definition Block bytes carried by the DSDT.
    #[must_use]
    pub fn aml_bytes(&self) -> &'a [u8] {
        self.table.payload()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_dsdt(aml: &[u8]) -> [u8; 40] {
        let mut bytes = [0_u8; 40];
        bytes[0..4].copy_from_slice(b"DSDT");
        bytes[4..8].copy_from_slice(&(40_u32).to_le_bytes());
        bytes[8] = 2;
        bytes[10..16].copy_from_slice(b"FUSION");
        bytes[16..24].copy_from_slice(b"DSDTTEST");
        bytes[36..40].copy_from_slice(aml);
        let checksum =
            (!bytes.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte))).wrapping_add(1);
        bytes[9] = checksum;
        bytes
    }

    #[test]
    fn dsdt_exposes_aml_payload() {
        let bytes = build_dsdt(&[0x10, 0x20, 0x30, 0x40]);
        let dsdt = Dsdt::parse(&bytes).expect("dsdt should parse");
        assert_eq!(dsdt.aml_bytes(), &[0x10, 0x20, 0x30, 0x40]);
    }
}
