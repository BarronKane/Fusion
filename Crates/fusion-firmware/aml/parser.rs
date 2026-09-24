//! AML parser configuration and parser anchor types.

use crate::aml::{
    AmlEncodedNameString,
    AmlPkgLength,
    AmlResult,
};

/// Parser strictness knobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlParserConfig {
    pub strict_names: bool,
    pub strict_pkg_lengths: bool,
    pub reject_unknown_ext_opcodes: bool,
}

impl Default for AmlParserConfig {
    fn default() -> Self {
        Self {
            strict_names: true,
            strict_pkg_lengths: true,
            reject_unknown_ext_opcodes: false,
        }
    }
}

/// Opaque AML parser anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct AmlParser;

impl AmlParser {
    ///
    /// # Errors
    ///
    /// Returns an error if the requested operation cannot be completed.
    pub fn parse_pkg_length(bytes: &[u8]) -> AmlResult<AmlPkgLength> {
        AmlPkgLength::parse(bytes)
    }

    ///
    /// # Errors
    ///
    /// Returns an error if the requested operation cannot be completed.
    pub fn parse_encoded_name_string(bytes: &[u8]) -> AmlResult<AmlEncodedNameString<'_>> {
        AmlEncodedNameString::parse(bytes)
    }
}
