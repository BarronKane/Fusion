//! XSDT definitions and helpers.
//!
//! The Extended System Description Table (`XSDT`) is the 64-bit root of the
//! modern ACPI table graph. In the UEFI Forum ACPI Specification 6.6, Section
//! 5.2.8 defines it as the `DESCRIPTION_HEADER` followed by an array of 64-bit
//! physical addresses pointing at other SDTs.
//!
//! A few standard rules matter directly here:
//!
//! - the `XSDT` supersedes the old `RSDT` when present,
//! - the payload begins immediately after the 36-byte common header,
//! - every entry is a 64-bit physical pointer to another table header,
//! - the OS is expected to follow those pointers and inspect the pointed-to
//!   signatures before interpreting the payload.
//!
//! This module therefore does one job: validate the envelope and expose the
//! entry array honestly. It does not resolve physical addresses by itself,
//! because that belongs to whatever memory-mapping path the HAL is using. A
//! parser that assumes direct physical access everywhere is how elegant code
//! turns into boot-time fiction.

use super::{
    AcpiError,
    AcpiSignature,
    AcpiTableView,
    read_unaligned_copy,
};

/// Borrowed validated XSDT view.
#[derive(Clone, Copy, Debug)]
pub struct Xsdt<'a> {
    table: AcpiTableView<'a>,
}

impl<'a> Xsdt<'a> {
    /// Parses one validated XSDT.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the table is malformed, truncated, or not one XSDT.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, AcpiError> {
        let table = AcpiTableView::parse_signature(bytes, AcpiSignature::XSDT)?;
        if table.payload().len() % size_of::<u64>() != 0 {
            return Err(AcpiError::invalid_layout());
        }
        Ok(Self { table })
    }

    /// Returns the underlying validated ACPI table view.
    #[must_use]
    pub const fn table(self) -> AcpiTableView<'a> {
        self.table
    }

    /// Returns the number of physical table pointers in the XSDT.
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.table.payload().len() / size_of::<u64>()
    }

    /// Returns one physical table pointer by index.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the entry bytes are malformed.
    pub fn entry(&self, index: usize) -> Result<Option<u64>, AcpiError> {
        let payload = self.table.payload();
        let start = index.saturating_mul(size_of::<u64>());
        let Some(entry_bytes) = payload.get(start..start + size_of::<u64>()) else {
            return Ok(None);
        };
        Ok(Some(u64::from_le(read_unaligned_copy::<u64>(entry_bytes)?)))
    }

    /// Returns an iterator over the XSDT's physical table pointers.
    #[must_use]
    pub fn entries(&self) -> XsdtEntryIter<'a> {
        XsdtEntryIter {
            payload: self.table.payload(),
            offset: 0,
        }
    }
}

/// Iterator over physical table addresses stored in one XSDT.
#[derive(Clone, Copy, Debug)]
pub struct XsdtEntryIter<'a> {
    payload: &'a [u8],
    offset: usize,
}

impl Iterator for XsdtEntryIter<'_> {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        let bytes = self
            .payload
            .get(self.offset..self.offset + size_of::<u64>())?;
        self.offset += size_of::<u64>();
        Some(u64::from_le(read_unaligned_copy::<u64>(bytes).ok()?))
    }
}

use core::mem::size_of;

#[cfg(test)]
mod tests {
    use super::*;

    fn build_xsdt(entries: &[u64]) -> [u8; 52] {
        let mut bytes = [0_u8; 52];
        bytes[0..4].copy_from_slice(b"XSDT");
        bytes[4..8].copy_from_slice(&(52_u32).to_le_bytes());
        bytes[8] = 1;
        bytes[10..16].copy_from_slice(b"FUSION");
        bytes[16..24].copy_from_slice(b"XSDTTEST");
        for (index, entry) in entries.iter().enumerate() {
            let start = 36 + index * 8;
            bytes[start..start + 8].copy_from_slice(&entry.to_le_bytes());
        }
        let checksum =
            (!bytes.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte))).wrapping_add(1);
        bytes[9] = checksum;
        bytes
    }

    #[test]
    fn xsdt_exposes_entry_iteration() {
        let bytes = build_xsdt(&[0x1000, 0x2000]);
        let xsdt = Xsdt::parse(&bytes).expect("xsdt should parse");
        assert_eq!(xsdt.entry_count(), 2);
        assert!(xsdt.entries().eq([0x1000, 0x2000]));
    }
}
