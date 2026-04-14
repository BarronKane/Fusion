//! AML bytecode and definition-block envelope types.

use crate::aml::{
    AmlError,
    AmlResult,
};
use crate::pal::hal::acpi::{
    AcpiTableView,
    Dsdt,
};

/// One AML definition block class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AmlDefinitionBlockKind {
    Dsdt,
    Ssdt,
    Psdt,
    Other([u8; 4]),
}

impl AmlDefinitionBlockKind {
    #[must_use]
    pub const fn from_signature(signature: [u8; 4]) -> Self {
        match signature {
            [b'D', b'S', b'D', b'T'] => Self::Dsdt,
            [b'S', b'S', b'D', b'T'] => Self::Ssdt,
            [b'P', b'S', b'D', b'T'] => Self::Psdt,
            other => Self::Other(other),
        }
    }

    #[must_use]
    pub const fn is_definition_block(self) -> bool {
        !matches!(self, Self::Other(_))
    }
}

/// Minimal identity for one AML definition block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlDefinitionBlockHeader {
    pub signature: [u8; 4],
    pub revision: u8,
    pub oem_id: [u8; 6],
    pub oem_table_id: [u8; 8],
    pub oem_revision: u32,
}

impl AmlDefinitionBlockHeader {
    #[must_use]
    pub const fn from_acpi_table(table: AcpiTableView<'_>) -> Self {
        let header = table.header();
        Self {
            signature: header.signature().bytes(),
            revision: header.revision(),
            oem_id: header.oem_id(),
            oem_table_id: header.oem_table_id(),
            oem_revision: header.oem_revision(),
        }
    }
}

/// Borrowed AML definition block bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmlDefinitionBlock<'a> {
    pub kind: AmlDefinitionBlockKind,
    pub header: AmlDefinitionBlockHeader,
    pub bytes: &'a [u8],
}

impl<'a> AmlDefinitionBlock<'a> {
    pub fn from_acpi_table(table: AcpiTableView<'a>) -> AmlResult<Self> {
        let kind = AmlDefinitionBlockKind::from_signature(table.header().signature().bytes());
        if !kind.is_definition_block() {
            return Err(AmlError::invalid_definition_block());
        }

        Ok(Self {
            kind,
            header: AmlDefinitionBlockHeader::from_acpi_table(table),
            bytes: table.payload(),
        })
    }

    pub fn from_dsdt(dsdt: Dsdt<'a>) -> AmlResult<Self> {
        Self::from_acpi_table(dsdt.table())
    }

    #[must_use]
    pub const fn signature(self) -> [u8; 4] {
        self.header.signature
    }
}

/// One bytecode span inside a definition block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlBytecodeSpan {
    pub offset: u32,
    pub length: u32,
}

impl AmlBytecodeSpan {
    #[must_use]
    pub const fn end_offset(self) -> u32 {
        self.offset.saturating_add(self.length)
    }

    #[must_use]
    pub const fn contains(self, offset: u32) -> bool {
        offset >= self.offset && offset < self.end_offset()
    }
}

/// Stable location of one AML bytecode region inside a loaded definition-block set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlCodeLocation {
    pub block_index: u16,
    pub span: AmlBytecodeSpan,
}

/// AML package-length envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlPkgLength {
    pub value: u32,
    pub encoded_bytes: u8,
}

impl AmlPkgLength {
    pub fn parse(bytes: &[u8]) -> AmlResult<Self> {
        let lead = *bytes.first().ok_or_else(AmlError::truncated)?;
        let follow_count = (lead >> 6) & 0b11;
        let encoded_bytes = follow_count + 1;
        if bytes.len() < usize::from(encoded_bytes) {
            return Err(AmlError::truncated());
        }

        let value = if follow_count == 0 {
            u32::from(lead & 0x3f)
        } else {
            let mut value = u32::from(lead & 0x0f);
            let mut shift = 4_u32;
            let mut index = 1_u8;
            while index <= follow_count {
                value |= u32::from(bytes[usize::from(index)]) << shift;
                shift += 8;
                index += 1;
            }
            value
        };

        Ok(Self {
            value,
            encoded_bytes,
        })
    }

    #[must_use]
    pub const fn payload_span(self, offset: u32) -> AmlBytecodeSpan {
        AmlBytecodeSpan {
            offset: offset + (self.encoded_bytes as u32),
            length: self.value.saturating_sub(self.encoded_bytes as u32),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aml::AmlErrorKind;
    use crate::pal::hal::acpi::AcpiSignature;

    fn build_definition_block(signature: [u8; 4], revision: u8, payload: &[u8]) -> [u8; 44] {
        let mut bytes = [0_u8; 44];
        bytes[0..4].copy_from_slice(&signature);
        bytes[4..8].copy_from_slice(&(44_u32).to_le_bytes());
        bytes[8] = revision;
        bytes[10..16].copy_from_slice(b"FUSION");
        bytes[16..24].copy_from_slice(b"AMLBLOCK");
        bytes[24..28].copy_from_slice(&(1_u32).to_le_bytes());
        bytes[28..32].copy_from_slice(&(2_u32).to_le_bytes());
        bytes[32..36].copy_from_slice(&(3_u32).to_le_bytes());
        bytes[36..44].copy_from_slice(payload);
        let checksum =
            (!bytes.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte))).wrapping_add(1);
        bytes[9] = checksum;
        bytes
    }

    #[test]
    fn definition_block_parses_from_dsdt() {
        let bytes = build_definition_block(*b"DSDT", 2, &[0x10, 0x20, 0x30, 0x40, 0, 0, 0, 0]);
        let dsdt = Dsdt::parse(&bytes).expect("dsdt should parse");
        let block = AmlDefinitionBlock::from_dsdt(dsdt).expect("definition block should parse");
        assert_eq!(block.kind, AmlDefinitionBlockKind::Dsdt);
        assert_eq!(block.signature(), *b"DSDT");
        assert_eq!(block.bytes[0..4], [0x10, 0x20, 0x30, 0x40]);
    }

    #[test]
    fn definition_block_rejects_non_definition_tables() {
        let bytes = build_definition_block(*b"XSDT", 1, &[0; 8]);
        let table =
            AcpiTableView::parse_signature(&bytes, AcpiSignature::XSDT).expect("xsdt should parse");
        let error = AmlDefinitionBlock::from_acpi_table(table).unwrap_err();
        assert_eq!(error.kind, AmlErrorKind::InvalidDefinitionBlock);
    }

    #[test]
    fn pkg_length_parses_one_byte_encoding() {
        let length = AmlPkgLength::parse(&[0x3f]).expect("pkg length should parse");
        assert_eq!(length.value, 0x3f);
        assert_eq!(length.encoded_bytes, 1);
    }

    #[test]
    fn pkg_length_parses_multi_byte_encoding() {
        let length = AmlPkgLength::parse(&[0b0100_1010, 0x23]).expect("pkg length should parse");
        assert_eq!(length.encoded_bytes, 2);
        assert_eq!(length.value, 0x23a);
        assert_eq!(
            length.payload_span(10),
            AmlBytecodeSpan {
                offset: 12,
                length: 568,
            }
        );
    }
}
