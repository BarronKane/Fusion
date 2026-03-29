//! MCFG definitions and helpers.
//!
//! `MCFG` is ACPI-adjacent in the most standards-body way possible.
//!
//! The UEFI Forum ACPI Specification 6.6 reserves the `MCFG` signature in the
//! SDT signature registry and identifies it as the PCI Express memory-mapped
//! configuration-space base-address description table, but the actual payload
//! definition is delegated to the PCI Firmware Specification rather than fully
//! specified by ACPI itself.
//!
//! Practically, that still gives Fusion one clear contract to implement:
//!
//! - treat `MCFG` as an ACPI SDT with the normal common header,
//! - validate the table checksum and length like any other SDT,
//! - parse the fixed reserved prefix,
//! - expose the ECAM allocation descriptors that map PCI segment and bus ranges
//!   to physical configuration-space windows.
//!
//! This is enough for early PCIe topology discovery. Anything beyond that,
//! such as actual bus walks, config-space probing, or driver binding, belongs
//! to later layers that consume these allocations instead of pretending that
//! the table itself is the whole PCI subsystem.

use core::mem::size_of;

use super::{AcpiError, AcpiSignature, AcpiTableView, read_unaligned_copy};

#[derive(Clone, Copy, Debug)]
#[repr(C)]
struct RawMcfgHeader {
    reserved: u64,
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
struct RawMcfgAllocation {
    base_address: u64,
    segment_group: u16,
    start_bus: u8,
    end_bus: u8,
    reserved: u32,
}

/// One PCIe enhanced-configuration allocation entry from MCFG.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct McfgAllocation {
    /// Base address of the enhanced-configuration window.
    pub base_address: u64,
    /// PCI segment group.
    pub segment_group: u16,
    /// Inclusive start bus.
    pub start_bus: u8,
    /// Inclusive end bus.
    pub end_bus: u8,
}

/// Borrowed validated MCFG view.
#[derive(Clone, Copy, Debug)]
pub struct Mcfg<'a> {
    table: AcpiTableView<'a>,
    reserved: u64,
}

impl<'a> Mcfg<'a> {
    /// Parses one validated MCFG.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the table is malformed, truncated, or not one MCFG.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, AcpiError> {
        let table = AcpiTableView::parse_signature(bytes, AcpiSignature::MCFG)?;
        let payload = table.payload();
        if payload.len() < size_of::<RawMcfgHeader>() {
            return Err(AcpiError::truncated());
        }
        let header: RawMcfgHeader = read_unaligned_copy(payload)?;
        let allocations = &payload[size_of::<RawMcfgHeader>()..];
        if allocations.len() % size_of::<RawMcfgAllocation>() != 0 {
            return Err(AcpiError::invalid_layout());
        }
        Ok(Self {
            table,
            reserved: u64::from_le(header.reserved),
        })
    }

    /// Returns the underlying validated ACPI table view.
    #[must_use]
    pub const fn table(self) -> AcpiTableView<'a> {
        self.table
    }

    /// Returns the reserved header field.
    #[must_use]
    pub const fn reserved(self) -> u64 {
        self.reserved
    }

    /// Returns the number of configuration-space allocation entries.
    #[must_use]
    pub fn allocation_count(&self) -> usize {
        self.allocations_bytes().len() / size_of::<RawMcfgAllocation>()
    }

    /// Returns one allocation entry by index.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the entry bytes are malformed.
    pub fn allocation(&self, index: usize) -> Result<Option<McfgAllocation>, AcpiError> {
        let start = index.saturating_mul(size_of::<RawMcfgAllocation>());
        let Some(bytes) = self
            .allocations_bytes()
            .get(start..start + size_of::<RawMcfgAllocation>())
        else {
            return Ok(None);
        };
        Ok(Some(parse_allocation(bytes)?))
    }

    /// Returns an iterator over MCFG allocation entries.
    #[must_use]
    pub fn allocations(&self) -> McfgAllocationIter<'a> {
        McfgAllocationIter {
            bytes: self.allocations_bytes(),
            offset: 0,
        }
    }

    fn allocations_bytes(&self) -> &'a [u8] {
        &self.table.payload()[size_of::<RawMcfgHeader>()..]
    }
}

/// Iterator over MCFG allocation entries.
#[derive(Clone, Copy, Debug)]
pub struct McfgAllocationIter<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl Iterator for McfgAllocationIter<'_> {
    type Item = McfgAllocation;

    fn next(&mut self) -> Option<Self::Item> {
        let bytes = self
            .bytes
            .get(self.offset..self.offset + size_of::<RawMcfgAllocation>())?;
        self.offset += size_of::<RawMcfgAllocation>();
        parse_allocation(bytes).ok()
    }
}

fn parse_allocation(bytes: &[u8]) -> Result<McfgAllocation, AcpiError> {
    let raw: RawMcfgAllocation = read_unaligned_copy(bytes)?;
    Ok(McfgAllocation {
        base_address: u64::from_le(raw.base_address),
        segment_group: u16::from_le(raw.segment_group),
        start_bus: raw.start_bus,
        end_bus: raw.end_bus,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_mcfg() -> [u8; 60] {
        let mut bytes = [0_u8; 60];
        bytes[0..4].copy_from_slice(b"MCFG");
        bytes[4..8].copy_from_slice(&(60_u32).to_le_bytes());
        bytes[8] = 1;
        bytes[10..16].copy_from_slice(b"FUSION");
        bytes[16..24].copy_from_slice(b"MCFGTEST");
        let entry_offset = 44;
        bytes[entry_offset..entry_offset + 8].copy_from_slice(&0xE000_0000_u64.to_le_bytes());
        bytes[entry_offset + 8..entry_offset + 10].copy_from_slice(&3_u16.to_le_bytes());
        bytes[entry_offset + 10] = 0;
        bytes[entry_offset + 11] = 31;
        let checksum =
            (!bytes.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte))).wrapping_add(1);
        bytes[9] = checksum;
        bytes
    }

    #[test]
    fn mcfg_exposes_config_allocations() {
        let bytes = build_mcfg();
        let mcfg = Mcfg::parse(&bytes).expect("mcfg should parse");
        let allocation = mcfg
            .allocation(0)
            .expect("allocation bytes should be valid")
            .expect("first allocation should exist");
        assert_eq!(mcfg.allocation_count(), 1);
        assert_eq!(allocation.base_address, 0xE000_0000);
        assert_eq!(allocation.segment_group, 3);
        assert_eq!(allocation.start_bus, 0);
        assert_eq!(allocation.end_bus, 31);
    }
}
