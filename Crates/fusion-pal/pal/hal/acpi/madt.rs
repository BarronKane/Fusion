//! MADT definitions and helpers.
//!
//! The Multiple APIC Description Table (`MADT`, signature `APIC`) is ACPI's
//! interrupt-topology handoff. In the UEFI Forum ACPI Specification 6.6,
//! Section 5.2.12 defines it as the table that tells OSPM which interrupt
//! controller model the platform exposes and how the machine's interrupt lines
//! map into ACPI's Global System Interrupt space.
//!
//! The MADT starts with the normal common SDT header, then a small fixed
//! header carrying:
//!
//! - the local interrupt-controller base address,
//! - table-level flags such as `PCAT_COMPAT`,
//! - a variable-length stream of interrupt-controller records.
//!
//! ACPI 6.6 permits a fairly absurd variety of record types here: classic APIC
//! and x2APIC structures, GIC structures, RISC-V interrupt-controller
//! structures, and more. Fusion only parses the subset needed for the first
//! x86-style bring-up path:
//!
//! - Processor Local APIC,
//! - I/O APIC,
//! - Interrupt Source Override,
//! - Local APIC NMI,
//! - Local APIC Address Override,
//! - Processor Local x2APIC.
//!
//! Unknown record types are preserved as opaque borrowed payloads so the parser
//! stays forward-compatible instead of exploding the moment firmware remembers
//! other architectures exist. Also, several MADT record bodies are byte-packed
//! on the wire, so the raw record structs here use packed layouts. One polite
//! padding byte in the wrong place would turn interrupt topology into a work of
//! fiction, and the firmware is already doing enough of that on its own.

use core::mem::size_of;

use bitflags::bitflags;

use super::{AcpiError, AcpiSignature, AcpiTableView, read_unaligned_copy};

bitflags! {
    /// MADT table-level flags.
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub struct MadtFlags: u32 {
        /// Legacy dual-8259 PICs are present.
        const PCAT_COMPAT = 1 << 0;
    }
}

bitflags! {
    /// Processor local-APIC enablement flags.
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub struct MadtLocalApicFlags: u32 {
        /// Processor is usable immediately.
        const ENABLED = 1 << 0;
        /// Processor may be brought online later.
        const ONLINE_CAPABLE = 1 << 1;
    }
}

/// Interrupt-source polarity from MADT flags.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MadtInterruptPolarity {
    ConformsToBus,
    ActiveHigh,
    Reserved,
    ActiveLow,
}

/// Interrupt trigger mode from MADT flags.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MadtInterruptTriggerMode {
    ConformsToBus,
    Edge,
    Reserved,
    Level,
}

/// Decoded MADT interrupt-source flags.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MadtInterruptFlags {
    raw: u16,
}

impl MadtInterruptFlags {
    /// Creates one decoded flag wrapper from the raw MADT bits.
    #[must_use]
    pub const fn new(raw: u16) -> Self {
        Self { raw }
    }

    /// Returns the raw flags.
    #[must_use]
    pub const fn raw(self) -> u16 {
        self.raw
    }

    /// Returns the interrupt-source polarity.
    #[must_use]
    pub const fn polarity(self) -> MadtInterruptPolarity {
        match self.raw & 0b11 {
            0 => MadtInterruptPolarity::ConformsToBus,
            1 => MadtInterruptPolarity::ActiveHigh,
            2 => MadtInterruptPolarity::Reserved,
            _ => MadtInterruptPolarity::ActiveLow,
        }
    }

    /// Returns the interrupt trigger mode.
    #[must_use]
    pub const fn trigger_mode(self) -> MadtInterruptTriggerMode {
        match (self.raw >> 2) & 0b11 {
            0 => MadtInterruptTriggerMode::ConformsToBus,
            1 => MadtInterruptTriggerMode::Edge,
            2 => MadtInterruptTriggerMode::Reserved,
            _ => MadtInterruptTriggerMode::Level,
        }
    }
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
struct RawMadtHeader {
    local_apic_address: u32,
    flags: u32,
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
struct RawMadtRecordHeader {
    kind: u8,
    length: u8,
}

#[derive(Clone, Copy, Debug)]
#[repr(C, packed)]
struct RawProcessorLocalApic {
    processor_uid: u8,
    apic_id: u8,
    flags: u32,
}

#[derive(Clone, Copy, Debug)]
#[repr(C, packed)]
struct RawIoApic {
    io_apic_id: u8,
    reserved: u8,
    io_apic_address: u32,
    global_system_interrupt_base: u32,
}

#[derive(Clone, Copy, Debug)]
#[repr(C, packed)]
struct RawInterruptSourceOverride {
    bus: u8,
    source: u8,
    global_system_interrupt: u32,
    flags: u16,
}

#[derive(Clone, Copy, Debug)]
#[repr(C, packed)]
struct RawLocalApicNmi {
    processor_uid: u8,
    flags: u16,
    lint: u8,
}

#[derive(Clone, Copy, Debug)]
#[repr(C, packed)]
struct RawLocalApicAddressOverride {
    reserved: u16,
    local_apic_address: u64,
}

#[derive(Clone, Copy, Debug)]
#[repr(C, packed)]
struct RawProcessorLocalX2Apic {
    reserved: u16,
    x2apic_id: u32,
    flags: u32,
    processor_uid: u32,
}

/// Parsed processor-local APIC record.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MadtProcessorLocalApic {
    pub processor_uid: u8,
    pub apic_id: u8,
    pub flags: MadtLocalApicFlags,
}

/// Parsed I/O APIC record.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MadtIoApic {
    pub io_apic_id: u8,
    pub io_apic_address: u32,
    pub global_system_interrupt_base: u32,
}

/// Parsed interrupt-source override record.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MadtInterruptSourceOverride {
    pub bus: u8,
    pub source: u8,
    pub global_system_interrupt: u32,
    pub flags: MadtInterruptFlags,
}

/// Parsed local-APIC NMI record.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MadtLocalApicNmi {
    pub processor_uid: u8,
    pub flags: MadtInterruptFlags,
    pub lint: u8,
}

/// Parsed local-APIC address override record.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MadtLocalApicAddressOverride {
    pub local_apic_address: u64,
}

/// Parsed processor-local x2APIC record.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MadtProcessorLocalX2Apic {
    pub x2apic_id: u32,
    pub flags: MadtLocalApicFlags,
    pub processor_uid: u32,
}

/// Borrowed parsed MADT record view.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MadtRecord<'a> {
    ProcessorLocalApic(MadtProcessorLocalApic),
    IoApic(MadtIoApic),
    InterruptSourceOverride(MadtInterruptSourceOverride),
    LocalApicNmi(MadtLocalApicNmi),
    LocalApicAddressOverride(MadtLocalApicAddressOverride),
    ProcessorLocalX2Apic(MadtProcessorLocalX2Apic),
    Unknown { kind: u8, bytes: &'a [u8] },
}

/// Borrowed validated MADT view.
#[derive(Clone, Copy, Debug)]
pub struct Madt<'a> {
    table: AcpiTableView<'a>,
    local_apic_address: u32,
    flags: MadtFlags,
}

impl<'a> Madt<'a> {
    /// Parses one validated MADT.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the table is malformed, truncated, or not one MADT.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, AcpiError> {
        let table = AcpiTableView::parse_signature(bytes, AcpiSignature::MADT)?;
        let payload = table.payload();
        if payload.len() < size_of::<RawMadtHeader>() {
            return Err(AcpiError::truncated());
        }
        let header: RawMadtHeader = read_unaligned_copy(payload)?;
        let raw_flags = u32::from_le(header.flags);
        if raw_flags & !MadtFlags::PCAT_COMPAT.bits() != 0 {
            return Err(AcpiError::invalid_layout());
        }
        Ok(Self {
            table,
            local_apic_address: u32::from_le(header.local_apic_address),
            flags: MadtFlags::from_bits_retain(raw_flags),
        })
    }

    /// Returns the underlying validated ACPI table view.
    #[must_use]
    pub const fn table(self) -> AcpiTableView<'a> {
        self.table
    }

    /// Returns the 32-bit local APIC base address from the MADT header.
    #[must_use]
    pub const fn local_apic_address(self) -> u32 {
        self.local_apic_address
    }

    /// Returns the effective local APIC address, honoring any override record.
    ///
    /// ACPI 6.6 Section 5.2.12.8 requires OSPM to use the Local APIC Address
    /// Override Structure when present instead of the 32-bit header field.
    ///
    /// # Errors
    ///
    /// Returns one honest error when record parsing fails or when the MADT
    /// contains more than one Local APIC Address Override structure.
    pub fn effective_local_apic_address(&self) -> Result<u64, AcpiError> {
        let mut override_address = None;
        for record in self.records() {
            let MadtRecord::LocalApicAddressOverride(record) = record? else {
                continue;
            };
            if override_address
                .replace(record.local_apic_address)
                .is_some()
            {
                return Err(AcpiError::invalid_layout());
            }
        }
        Ok(override_address.unwrap_or(u64::from(self.local_apic_address)))
    }

    /// Returns the MADT table flags.
    #[must_use]
    pub const fn flags(self) -> MadtFlags {
        self.flags
    }

    /// Returns an iterator over MADT variable-length records.
    #[must_use]
    pub fn records(&self) -> MadtRecordIter<'a> {
        MadtRecordIter {
            bytes: &self.table.payload()[size_of::<RawMadtHeader>()..],
            offset: 0,
        }
    }
}

/// Iterator over MADT variable-length records.
#[derive(Clone, Copy, Debug)]
pub struct MadtRecordIter<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Iterator for MadtRecordIter<'a> {
    type Item = Result<MadtRecord<'a>, AcpiError>;

    fn next(&mut self) -> Option<Self::Item> {
        let header_bytes = self
            .bytes
            .get(self.offset..self.offset + size_of::<RawMadtRecordHeader>())?;
        let header: RawMadtRecordHeader = match read_unaligned_copy(header_bytes) {
            Ok(header) => header,
            Err(error) => return Some(Err(error)),
        };
        if usize::from(header.length) < size_of::<RawMadtRecordHeader>() {
            return Some(Err(AcpiError::invalid_layout()));
        }
        let end = self.offset + usize::from(header.length);
        let Some(record_bytes) = self.bytes.get(self.offset..end) else {
            return Some(Err(AcpiError::truncated()));
        };
        self.offset = end;
        Some(parse_record(
            header.kind,
            &record_bytes[size_of::<RawMadtRecordHeader>()..],
        ))
    }
}

fn parse_record_body<T: Copy>(payload: &[u8]) -> Result<T, AcpiError> {
    if payload.len() != size_of::<T>() {
        return Err(AcpiError::invalid_layout());
    }
    read_unaligned_copy(payload)
}

fn parse_local_apic_flags(raw: u32) -> Result<MadtLocalApicFlags, AcpiError> {
    let enabled = MadtLocalApicFlags::ENABLED.bits();
    let online_capable = MadtLocalApicFlags::ONLINE_CAPABLE.bits();
    let defined = enabled | online_capable;
    if raw & !defined != 0 {
        return Err(AcpiError::invalid_layout());
    }
    if raw & enabled != 0 && raw & online_capable != 0 {
        return Err(AcpiError::invalid_layout());
    }
    Ok(MadtLocalApicFlags::from_bits_retain(raw))
}

fn parse_mps_inti_flags(raw: u16) -> Result<MadtInterruptFlags, AcpiError> {
    if raw & !0x000F != 0 {
        return Err(AcpiError::invalid_layout());
    }
    Ok(MadtInterruptFlags::new(raw))
}

fn parse_record<'a>(kind: u8, payload: &'a [u8]) -> Result<MadtRecord<'a>, AcpiError> {
    match kind {
        0 => {
            let raw: RawProcessorLocalApic = parse_record_body(payload)?;
            Ok(MadtRecord::ProcessorLocalApic(MadtProcessorLocalApic {
                processor_uid: raw.processor_uid,
                apic_id: raw.apic_id,
                flags: parse_local_apic_flags(u32::from_le(raw.flags))?,
            }))
        }
        1 => {
            let raw: RawIoApic = parse_record_body(payload)?;
            if raw.reserved != 0 {
                return Err(AcpiError::invalid_layout());
            }
            Ok(MadtRecord::IoApic(MadtIoApic {
                io_apic_id: raw.io_apic_id,
                io_apic_address: u32::from_le(raw.io_apic_address),
                global_system_interrupt_base: u32::from_le(raw.global_system_interrupt_base),
            }))
        }
        2 => {
            let raw: RawInterruptSourceOverride = parse_record_body(payload)?;
            if raw.bus != 0 {
                return Err(AcpiError::invalid_layout());
            }
            Ok(MadtRecord::InterruptSourceOverride(
                MadtInterruptSourceOverride {
                    bus: raw.bus,
                    source: raw.source,
                    global_system_interrupt: u32::from_le(raw.global_system_interrupt),
                    flags: parse_mps_inti_flags(u16::from_le(raw.flags))?,
                },
            ))
        }
        4 => {
            let raw: RawLocalApicNmi = parse_record_body(payload)?;
            Ok(MadtRecord::LocalApicNmi(MadtLocalApicNmi {
                processor_uid: raw.processor_uid,
                flags: parse_mps_inti_flags(u16::from_le(raw.flags))?,
                lint: raw.lint,
            }))
        }
        5 => {
            let raw: RawLocalApicAddressOverride = parse_record_body(payload)?;
            if raw.reserved != 0 {
                return Err(AcpiError::invalid_layout());
            }
            Ok(MadtRecord::LocalApicAddressOverride(
                MadtLocalApicAddressOverride {
                    local_apic_address: u64::from_le(raw.local_apic_address),
                },
            ))
        }
        9 => {
            let raw: RawProcessorLocalX2Apic = parse_record_body(payload)?;
            if raw.reserved != 0 {
                return Err(AcpiError::invalid_layout());
            }
            Ok(MadtRecord::ProcessorLocalX2Apic(MadtProcessorLocalX2Apic {
                x2apic_id: u32::from_le(raw.x2apic_id),
                flags: parse_local_apic_flags(u32::from_le(raw.flags))?,
                processor_uid: u32::from_le(raw.processor_uid),
            }))
        }
        _ => Ok(MadtRecord::Unknown {
            kind,
            bytes: payload,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::super::AcpiErrorKind;
    use super::*;
    use std::vec::Vec;

    fn build_madt() -> Vec<u8> {
        let mut bytes = vec![0_u8; 64];
        bytes[0..4].copy_from_slice(b"APIC");
        bytes[4..8].copy_from_slice(&(64_u32).to_le_bytes());
        bytes[8] = 1;
        bytes[10..16].copy_from_slice(b"FUSION");
        bytes[16..24].copy_from_slice(b"MADTTEST");
        bytes[36..40].copy_from_slice(&0xFEE0_0000_u32.to_le_bytes());
        bytes[40..44].copy_from_slice(&1_u32.to_le_bytes());

        bytes[44] = 0;
        bytes[45] = 8;
        bytes[46] = 7;
        bytes[47] = 9;
        bytes[48..52].copy_from_slice(&1_u32.to_le_bytes());

        bytes[52] = 1;
        bytes[53] = 12;
        bytes[54] = 2;
        bytes[55] = 0;
        bytes[56..60].copy_from_slice(&0xFEC0_0000_u32.to_le_bytes());
        bytes[60..64].copy_from_slice(&0_u32.to_le_bytes());

        let checksum =
            (!bytes.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte))).wrapping_add(1);
        bytes[9] = checksum;
        bytes
    }

    #[test]
    fn madt_surfaces_header_and_records() {
        let bytes = build_madt();
        let madt = Madt::parse(&bytes).expect("madt should parse");
        assert_eq!(madt.local_apic_address(), 0xFEE0_0000);
        assert_eq!(
            madt.effective_local_apic_address()
                .expect("no override should still produce one address"),
            0xFEE0_0000
        );
        assert!(madt.flags().contains(MadtFlags::PCAT_COMPAT));
        let records = madt
            .records()
            .collect::<Result<Vec<_>, _>>()
            .expect("records should parse");
        assert_eq!(records.len(), 2);
        assert!(matches!(
            records[0],
            MadtRecord::ProcessorLocalApic(MadtProcessorLocalApic {
                processor_uid: 7,
                apic_id: 9,
                ..
            })
        ));
        assert!(matches!(
            records[1],
            MadtRecord::IoApic(MadtIoApic {
                io_apic_id: 2,
                io_apic_address: 0xFEC0_0000,
                global_system_interrupt_base: 0,
            })
        ));
    }

    #[test]
    fn madt_prefers_local_apic_override_when_present() {
        let mut bytes = vec![0_u8; 56];
        bytes[0..4].copy_from_slice(b"APIC");
        bytes[4..8].copy_from_slice(&(56_u32).to_le_bytes());
        bytes[8] = 1;
        bytes[10..16].copy_from_slice(b"FUSION");
        bytes[16..24].copy_from_slice(b"MADTOVRD");
        bytes[36..40].copy_from_slice(&0xFEE0_0000_u32.to_le_bytes());
        bytes[40..44].copy_from_slice(&0_u32.to_le_bytes());
        bytes[44] = 5;
        bytes[45] = 12;
        bytes[46..48].copy_from_slice(&0_u16.to_le_bytes());
        bytes[48..56].copy_from_slice(&0x0000_0001_FEE0_0000_u64.to_le_bytes());
        let checksum =
            (!bytes.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte))).wrapping_add(1);
        bytes[9] = checksum;

        let madt = Madt::parse(&bytes).expect("madt should parse");
        assert_eq!(
            madt.effective_local_apic_address()
                .expect("override should parse"),
            0x0000_0001_FEE0_0000
        );
    }

    #[test]
    fn madt_rejects_supported_record_with_wrong_length() {
        let mut bytes = build_madt();
        bytes.push(0);
        let table_len = bytes.len() as u32;
        bytes[4..8].copy_from_slice(&table_len.to_le_bytes());
        bytes[53] = 13;
        bytes[9] = 0;
        let checksum =
            (!bytes.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte))).wrapping_add(1);
        bytes[9] = checksum;

        let madt = Madt::parse(&bytes).expect("madt should still parse");
        let error = madt
            .records()
            .nth(1)
            .expect("second record should exist")
            .expect_err("wrong-length io apic record should be rejected");
        assert_eq!(error.kind(), AcpiErrorKind::InvalidLayout);
    }
}
