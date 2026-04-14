//! AML field-unit descriptors.

use crate::aml::AmlNamespaceNodeId;

/// Declared field access granularity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AmlFieldAccessKind {
    Any,
    Byte,
    Word,
    DWord,
    QWord,
    Buffer,
}

/// Read-modify-write update rule for one field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AmlFieldUpdateKind {
    Preserve,
    WriteAsOnes,
    WriteAsZeros,
}

/// Stable descriptor for one AML field unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlFieldDescriptor {
    pub node: AmlNamespaceNodeId,
    pub region: Option<AmlNamespaceNodeId>,
    pub bit_offset: u32,
    pub bit_width: u32,
    pub access: AmlFieldAccessKind,
    pub update: AmlFieldUpdateKind,
}
