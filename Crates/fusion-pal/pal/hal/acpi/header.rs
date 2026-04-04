//! Common ACPI SDT header and generic table view support.
//!
//! ACPI System Description Tables share one fixed header shape before their
//! table-specific payload begins. In the UEFI Forum ACPI Specification 6.6,
//! that is the `DESCRIPTION_HEADER` defined in Section 5.2.6. Its fields are
//! the first admissibility gate for every SDT:
//!
//! - `Signature` tells OSPM what kind of table it found,
//! - `Length` bounds the table in physical memory,
//! - `Revision` identifies the format revision for that signature,
//! - `Checksum` requires the whole table to sum to zero,
//! - OEM and creator metadata provide provenance and revision identity.
//!
//! Fusion uses that common contract exactly once here, then hands each payload
//! off to its table-specific parser. This keeps the generic rules centralized
//! and, more importantly, preserves ACPI's actual contract: unknown signatures
//! are not reinterpreted by wishful thinking, they are ignored unless some
//! higher layer explicitly knows how to consume them.

use core::fmt;
use core::mem::size_of;

use super::{
    AcpiError,
    checksum_is_valid,
    read_unaligned_copy,
};

/// Four-byte ACPI table signature.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct AcpiSignature([u8; 4]);

impl AcpiSignature {
    /// DSDT signature.
    pub const DSDT: Self = Self(*b"DSDT");
    /// FADT signature (`FACP` on the wire).
    pub const FADT: Self = Self(*b"FACP");
    /// FACS signature.
    pub const FACS: Self = Self(*b"FACS");
    /// XSDT signature.
    pub const XSDT: Self = Self(*b"XSDT");
    /// MCFG signature.
    pub const MCFG: Self = Self(*b"MCFG");
    /// MADT signature.
    pub const MADT: Self = Self(*b"APIC");

    /// Creates one signature from four bytes.
    #[must_use]
    pub const fn new(bytes: [u8; 4]) -> Self {
        Self(bytes)
    }

    /// Returns the raw signature bytes.
    #[must_use]
    pub const fn bytes(self) -> [u8; 4] {
        self.0
    }
}

impl fmt::Debug for AcpiSignature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}{}{}",
            char::from(self.0[0]),
            char::from(self.0[1]),
            char::from(self.0[2]),
            char::from(self.0[3]),
        )
    }
}

impl fmt::Display for AcpiSignature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
struct RawAcpiSdtHeader {
    signature: [u8; 4],
    length: u32,
    revision: u8,
    checksum: u8,
    oem_id: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: u32,
    creator_revision: u32,
}

/// Parsed ACPI SDT header.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct AcpiSdtHeader {
    signature: AcpiSignature,
    length: u32,
    revision: u8,
    checksum: u8,
    oem_id: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: u32,
    creator_revision: u32,
}

impl AcpiSdtHeader {
    /// Byte size of one ACPI SDT header.
    pub const SIZE: usize = size_of::<RawAcpiSdtHeader>();

    /// Parses one ACPI SDT header from the front of `bytes`.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the buffer is too short or the declared length is smaller
    /// than the SDT header itself.
    pub fn parse(bytes: &[u8]) -> Result<Self, AcpiError> {
        let raw: RawAcpiSdtHeader = read_unaligned_copy(bytes)?;
        let length = u32::from_le(raw.length);
        if usize::try_from(length)
            .ok()
            .filter(|length| *length >= Self::SIZE)
            .is_none()
        {
            return Err(AcpiError::invalid_layout());
        }

        Ok(Self {
            signature: AcpiSignature::new(raw.signature),
            length,
            revision: raw.revision,
            checksum: raw.checksum,
            oem_id: raw.oem_id,
            oem_table_id: raw.oem_table_id,
            oem_revision: u32::from_le(raw.oem_revision),
            creator_id: u32::from_le(raw.creator_id),
            creator_revision: u32::from_le(raw.creator_revision),
        })
    }

    /// Returns the table signature.
    #[must_use]
    pub const fn signature(self) -> AcpiSignature {
        self.signature
    }

    /// Returns the declared total table length in bytes.
    #[must_use]
    pub const fn length(self) -> u32 {
        self.length
    }

    /// Returns the ACPI revision byte.
    #[must_use]
    pub const fn revision(self) -> u8 {
        self.revision
    }

    /// Returns the checksum byte stored in the header.
    #[must_use]
    pub const fn checksum(self) -> u8 {
        self.checksum
    }

    /// Returns the OEM identifier bytes.
    #[must_use]
    pub const fn oem_id(self) -> [u8; 6] {
        self.oem_id
    }

    /// Returns the OEM table identifier bytes.
    #[must_use]
    pub const fn oem_table_id(self) -> [u8; 8] {
        self.oem_table_id
    }

    /// Returns the OEM revision.
    #[must_use]
    pub const fn oem_revision(self) -> u32 {
        self.oem_revision
    }

    /// Returns the creator identifier.
    #[must_use]
    pub const fn creator_id(self) -> u32 {
        self.creator_id
    }

    /// Returns the creator revision.
    #[must_use]
    pub const fn creator_revision(self) -> u32 {
        self.creator_revision
    }
}

/// Generic borrowed ACPI SDT view with validated header and checksum.
#[derive(Clone, Copy, Debug)]
pub struct AcpiTableView<'a> {
    header: AcpiSdtHeader,
    bytes: &'a [u8],
}

impl<'a> AcpiTableView<'a> {
    /// Parses and validates one ACPI table view.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the table bytes are truncated, malformed, or fail checksum.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, AcpiError> {
        let header = AcpiSdtHeader::parse(bytes)?;
        let declared_len =
            usize::try_from(header.length()).map_err(|_| AcpiError::invalid_layout())?;
        let table_bytes = bytes.get(..declared_len).ok_or_else(AcpiError::truncated)?;
        if !checksum_is_valid(table_bytes) {
            return Err(AcpiError::invalid_checksum());
        }
        Ok(Self {
            header,
            bytes: table_bytes,
        })
    }

    /// Parses and validates one ACPI table view with one required signature.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the signature does not match or validation fails.
    pub fn parse_signature(bytes: &'a [u8], signature: AcpiSignature) -> Result<Self, AcpiError> {
        let table = Self::parse(bytes)?;
        if table.header.signature() != signature {
            return Err(AcpiError::invalid_signature());
        }
        Ok(table)
    }

    /// Returns the parsed header.
    #[must_use]
    pub const fn header(self) -> AcpiSdtHeader {
        self.header
    }

    /// Returns the validated raw table bytes.
    #[must_use]
    pub const fn bytes(self) -> &'a [u8] {
        self.bytes
    }

    /// Returns the validated payload bytes after the SDT header.
    #[must_use]
    pub fn payload(&self) -> &'a [u8] {
        &self.bytes[AcpiSdtHeader::SIZE..]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_table(signature: [u8; 4], payload: &[u8]) -> [u8; 40] {
        let mut bytes = [0_u8; 40];
        bytes[0..4].copy_from_slice(&signature);
        bytes[4..8].copy_from_slice(&(40_u32).to_le_bytes());
        bytes[8] = 2;
        bytes[10..16].copy_from_slice(b"FUSION");
        bytes[16..24].copy_from_slice(b"TESTTABL");
        bytes[24..28].copy_from_slice(&(1_u32).to_le_bytes());
        bytes[28..32].copy_from_slice(&(2_u32).to_le_bytes());
        bytes[32..36].copy_from_slice(&(3_u32).to_le_bytes());
        bytes[36..40].copy_from_slice(payload);
        let checksum =
            (!bytes.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte))).wrapping_add(1);
        bytes[9] = checksum;
        bytes
    }

    #[test]
    fn table_view_parses_valid_header_and_payload() {
        let bytes = build_table(*b"TEST", &[0xAA, 0xBB, 0xCC, 0xDD]);
        let view = AcpiTableView::parse_signature(&bytes, AcpiSignature::new(*b"TEST"))
            .expect("table should parse");
        assert_eq!(view.header().signature(), AcpiSignature::new(*b"TEST"));
        assert_eq!(view.payload(), &[0xAA, 0xBB, 0xCC, 0xDD]);
    }
}
