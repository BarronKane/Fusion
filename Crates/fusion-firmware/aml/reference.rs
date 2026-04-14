//! AML runtime reference vocabulary.

use crate::aml::AmlNamespaceNodeId;

/// One AML reference flavor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AmlReferenceKind {
    NamespaceNode(AmlNamespaceNodeId),
    Arg(u8),
    Local(u8),
    Field(AmlNamespaceNodeId),
    Index {
        base: AmlNamespaceNodeId,
        index: u32,
    },
}

/// Opaque AML runtime reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlReference {
    pub kind: AmlReferenceKind,
}
