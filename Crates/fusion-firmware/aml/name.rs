//! AML name and path vocabulary.

use crate::aml::{
    AmlError,
    AmlResult,
};

pub const AML_MAX_PATH_SEGMENTS: usize = 16;

/// One AML 4-character name segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlNameSeg([u8; 4]);

impl AmlNameSeg {
    pub const BLANK: Self = Self(*b"____");

    pub fn from_bytes(bytes: [u8; 4]) -> AmlResult<Self> {
        if bytes
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'_')
        {
            Ok(Self(bytes))
        } else {
            Err(AmlError::invalid_name())
        }
    }

    #[must_use]
    pub const fn bytes(self) -> [u8; 4] {
        self.0
    }
}

/// One resolved AML namespace path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlResolvedNamePath {
    segment_count: u8,
    segments: [AmlNameSeg; AML_MAX_PATH_SEGMENTS],
}

impl AmlResolvedNamePath {
    #[must_use]
    pub const fn root() -> Self {
        Self {
            segment_count: 0,
            segments: [AmlNameSeg::BLANK; AML_MAX_PATH_SEGMENTS],
        }
    }

    #[must_use]
    pub const fn segment_count(self) -> u8 {
        self.segment_count
    }

    #[must_use]
    pub fn segment(self, index: u8) -> Option<AmlNameSeg> {
        if index >= self.segment_count {
            None
        } else {
            Some(self.segments[usize::from(index)])
        }
    }

    #[must_use]
    pub fn last_segment(self) -> Option<AmlNameSeg> {
        self.segment_count
            .checked_sub(1)
            .and_then(|index| self.segment(index))
    }

    #[must_use]
    pub fn parent(self) -> Option<Self> {
        if self.segment_count == 0 {
            return None;
        }

        let mut path = self;
        path.pop().ok()?;
        Some(path)
    }

    #[must_use]
    pub fn prefix(self, segment_count: u8) -> Option<Self> {
        if segment_count > self.segment_count {
            return None;
        }

        let mut path = Self::root();
        let mut index = 0_u8;
        while index < segment_count {
            path.push(self.segment(index)?).ok()?;
            index += 1;
        }
        Some(path)
    }

    pub fn push(&mut self, segment: AmlNameSeg) -> AmlResult<()> {
        if usize::from(self.segment_count) >= AML_MAX_PATH_SEGMENTS {
            return Err(AmlError::overflow());
        }

        self.segments[usize::from(self.segment_count)] = segment;
        self.segment_count += 1;
        Ok(())
    }

    pub fn pop(&mut self) -> AmlResult<()> {
        if self.segment_count == 0 {
            return Err(AmlError::invalid_name());
        }

        self.segment_count -= 1;
        self.segments[usize::from(self.segment_count)] = AmlNameSeg::BLANK;
        Ok(())
    }

    pub fn resolve(self, encoded: AmlEncodedNameString<'_>) -> AmlResult<Self> {
        if encoded.is_null {
            return Ok(self);
        }

        let mut path = match encoded.anchor {
            AmlNameAnchor::Root => Self::root(),
            AmlNameAnchor::ParentPrefix => self,
            AmlNameAnchor::Local => self,
        };

        if matches!(encoded.anchor, AmlNameAnchor::ParentPrefix) {
            let mut remaining = encoded.parent_prefixes;
            while remaining != 0 {
                path.pop()?;
                remaining -= 1;
            }
        }

        let mut index = 0_u8;
        while index < encoded.segment_count {
            path.push(encoded.segment(index).ok_or_else(AmlError::invalid_name)?)?;
            index += 1;
        }

        Ok(path)
    }

    pub fn parse_text(raw: &str) -> AmlResult<Self> {
        if !raw.is_ascii() || raw.is_empty() {
            return Err(AmlError::invalid_name());
        }

        let (mut path, body) = if let Some(rest) = raw.strip_prefix('\\') {
            (Self::root(), rest)
        } else {
            return Err(AmlError::invalid_name());
        };

        if body.is_empty() {
            return Ok(path);
        }

        for segment in body.split('.') {
            path.push(parse_text_segment(segment)?)?;
        }

        Ok(path)
    }
}

/// How one AML name string is anchored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AmlNameAnchor {
    Root,
    ParentPrefix,
    Local,
}

/// Borrowed AML name-string spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlNameString<'a> {
    pub anchor: AmlNameAnchor,
    pub raw: &'a str,
}

impl<'a> AmlNameString<'a> {
    pub fn new(raw: &'a str) -> AmlResult<Self> {
        if raw.is_empty() || !raw.is_ascii() {
            return Err(AmlError::invalid_name());
        }

        let anchor = if raw.starts_with('\\') {
            AmlNameAnchor::Root
        } else if raw.starts_with('^') {
            AmlNameAnchor::ParentPrefix
        } else {
            AmlNameAnchor::Local
        };

        Ok(Self { anchor, raw })
    }

    #[must_use]
    pub const fn is_absolute(self) -> bool {
        matches!(self.anchor, AmlNameAnchor::Root)
    }
}

/// Borrowed encoded AML name string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlEncodedNameString<'a> {
    pub anchor: AmlNameAnchor,
    pub parent_prefixes: u8,
    pub segment_count: u8,
    pub is_null: bool,
    raw_segments: &'a [u8],
    pub consumed_bytes: u8,
}

impl<'a> AmlEncodedNameString<'a> {
    pub fn parse(bytes: &'a [u8]) -> AmlResult<Self> {
        if bytes.is_empty() {
            return Err(AmlError::truncated());
        }

        let mut offset = 0_usize;
        let mut anchor = AmlNameAnchor::Local;
        let mut parent_prefixes = 0_u8;

        if bytes[offset] == b'\\' {
            anchor = AmlNameAnchor::Root;
            offset += 1;
        } else {
            while offset < bytes.len() && bytes[offset] == b'^' {
                anchor = AmlNameAnchor::ParentPrefix;
                parent_prefixes = parent_prefixes.saturating_add(1);
                offset += 1;
            }
        }

        let opcode = *bytes.get(offset).ok_or_else(AmlError::truncated)?;
        let (segment_count, raw_segments, consumed_tail, is_null) = match opcode {
            0x00 => (0_u8, &bytes[offset + 1..offset + 1], 1_usize, true),
            0x2e => {
                let start = offset + 1;
                let end = start + 8;
                let raw_segments = bytes.get(start..end).ok_or_else(AmlError::truncated)?;
                validate_namesegs(raw_segments)?;
                (2_u8, raw_segments, 9_usize, false)
            }
            0x2f => {
                let count = *bytes.get(offset + 1).ok_or_else(AmlError::truncated)?;
                let start = offset + 2;
                let end = start + (usize::from(count) * 4);
                let raw_segments = bytes.get(start..end).ok_or_else(AmlError::truncated)?;
                validate_namesegs(raw_segments)?;
                (count, raw_segments, 2 + raw_segments.len(), false)
            }
            _ => {
                let start = offset;
                let end = start + 4;
                let raw_segments = bytes.get(start..end).ok_or_else(AmlError::truncated)?;
                validate_namesegs(raw_segments)?;
                (1_u8, raw_segments, 4_usize, false)
            }
        };

        Ok(Self {
            anchor,
            parent_prefixes,
            segment_count,
            is_null,
            raw_segments,
            consumed_bytes: u8::try_from(offset + consumed_tail)
                .map_err(|_| AmlError::overflow())?,
        })
    }

    #[must_use]
    pub const fn raw_segments(self) -> &'a [u8] {
        self.raw_segments
    }

    #[must_use]
    pub fn segment(self, index: u8) -> Option<AmlNameSeg> {
        if index >= self.segment_count {
            return None;
        }

        let start = usize::from(index) * 4;
        let bytes = [
            self.raw_segments[start],
            self.raw_segments[start + 1],
            self.raw_segments[start + 2],
            self.raw_segments[start + 3],
        ];
        AmlNameSeg::from_bytes(bytes).ok()
    }
}

fn validate_namesegs(bytes: &[u8]) -> AmlResult<()> {
    if bytes.len() % 4 != 0 {
        return Err(AmlError::invalid_name());
    }

    for chunk in bytes.chunks_exact(4) {
        AmlNameSeg::from_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])?;
    }

    Ok(())
}

fn parse_text_segment(raw: &str) -> AmlResult<AmlNameSeg> {
    if raw.is_empty() || raw.len() > 4 {
        return Err(AmlError::invalid_name());
    }

    let mut bytes = [b'_'; 4];
    for (index, byte) in raw.bytes().enumerate() {
        if !byte.is_ascii_uppercase() && !byte.is_ascii_digit() && byte != b'_' {
            return Err(AmlError::invalid_name());
        }
        bytes[index] = byte;
    }

    AmlNameSeg::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoded_name_parses_single_segment() {
        let name = AmlEncodedNameString::parse(b"ABCD").expect("name should parse");
        assert_eq!(name.anchor, AmlNameAnchor::Local);
        assert_eq!(name.segment_count, 1);
        assert_eq!(name.consumed_bytes, 4);
        assert_eq!(name.segment(0).unwrap().bytes(), *b"ABCD");
    }

    #[test]
    fn encoded_name_parses_rooted_dual_segment() {
        let name = AmlEncodedNameString::parse(b"\\.\x41\x42\x43\x44\x45\x46\x47\x48")
            .expect("name should parse");
        assert_eq!(name.anchor, AmlNameAnchor::Root);
        assert_eq!(name.segment_count, 2);
        assert_eq!(name.consumed_bytes, 10);
        assert_eq!(name.segment(1).unwrap().bytes(), *b"EFGH");
    }

    #[test]
    fn encoded_name_parses_parent_prefixed_multi_segment() {
        let name = AmlEncodedNameString::parse(b"^^/\x02ABCDWXYZ").expect("name should parse");
        assert_eq!(name.anchor, AmlNameAnchor::ParentPrefix);
        assert_eq!(name.parent_prefixes, 2);
        assert_eq!(name.segment_count, 2);
        assert_eq!(name.consumed_bytes, 12);
    }

    #[test]
    fn encoded_name_parses_null_name() {
        let name = AmlEncodedNameString::parse(&[0x00]).expect("null name should parse");
        assert!(name.is_null);
        assert_eq!(name.segment_count, 0);
        assert_eq!(name.consumed_bytes, 1);
    }

    #[test]
    fn resolved_path_appends_and_resolves_parent_prefixes() {
        let mut root = AmlResolvedNamePath::root();
        root.push(AmlNameSeg::from_bytes(*b"_SB_").unwrap())
            .unwrap();
        root.push(AmlNameSeg::from_bytes(*b"PCI0").unwrap())
            .unwrap();

        let child = root
            .resolve(AmlEncodedNameString::parse(b"ABCD").unwrap())
            .expect("child should resolve");
        assert_eq!(child.segment_count(), 3);
        assert_eq!(child.last_segment().unwrap().bytes(), *b"ABCD");

        let parent = child
            .resolve(AmlEncodedNameString::parse(b"^WXYZ").unwrap())
            .expect("parent path should resolve");
        assert_eq!(parent.segment_count(), 3);
        assert_eq!(parent.last_segment().unwrap().bytes(), *b"WXYZ");
    }

    #[test]
    fn resolved_path_parses_textual_absolute_path() {
        let path = AmlResolvedNamePath::parse_text("\\_SB.AC._PSR").expect("path should parse");
        assert_eq!(path.segment_count(), 3);
        assert_eq!(path.segment(0).unwrap().bytes(), *b"_SB_");
        assert_eq!(path.segment(1).unwrap().bytes(), *b"AC__");
        assert_eq!(path.segment(2).unwrap().bytes(), *b"_PSR");
    }

    #[test]
    fn resolved_path_rejects_invalid_textual_segments() {
        let error = AmlResolvedNamePath::parse_text("\\_SB.PCI_Config").unwrap_err();
        assert_eq!(error, AmlError::invalid_name());
    }
}
