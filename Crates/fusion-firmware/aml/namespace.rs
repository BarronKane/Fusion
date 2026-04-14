//! AML namespace identity and descriptor vocabulary.

use crate::aml::{
    AmlCodeLocation,
    AmlEncodedNameString,
    AmlFieldDescriptor,
    AmlMethodDescriptor,
    AmlNameAnchor,
    AmlObjectKind,
    AmlOpRegionDescriptor,
    AmlResolvedNamePath,
    AmlResult,
};

/// Stable identity for one AML namespace node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlNamespaceNodeId(pub u32);

/// Borrowed descriptor for one namespace node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmlNamespaceNodeDescriptor {
    pub id: AmlNamespaceNodeId,
    pub parent: Option<AmlNamespaceNodeId>,
    pub kind: AmlObjectKind,
    pub path: AmlResolvedNamePath,
}

/// Extra loaded payload carried by one namespace node record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmlNamespaceNodePayload {
    None,
    NameInteger(u64),
    Method(AmlMethodDescriptor),
    OpRegion(AmlOpRegionDescriptor),
    Field(AmlFieldDescriptor),
}

/// One loaded namespace node record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmlNamespaceLoadRecord {
    pub descriptor: AmlNamespaceNodeDescriptor,
    pub body: Option<AmlCodeLocation>,
    pub payload: AmlNamespaceNodePayload,
}

/// Namespace loading state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AmlNamespaceState {
    Empty,
    DefinitionBlocksLoaded,
    RegionsRegistered,
    Initialized,
}

/// Opaque namespace anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlNamespace {
    pub state: AmlNamespaceState,
    pub node_count: u32,
}

/// Borrowed result of one namespace-loading pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmlLoadedNamespace<'records, 'blocks> {
    pub namespace: AmlNamespace,
    pub records: &'records [AmlNamespaceLoadRecord],
    pub blocks: crate::aml::AmlDefinitionBlockSet<'blocks>,
}

impl<'records, 'blocks> AmlLoadedNamespace<'records, 'blocks> {
    #[must_use]
    pub fn record(self, node: AmlNamespaceNodeId) -> Option<&'records AmlNamespaceLoadRecord> {
        self.records
            .iter()
            .find(|record| record.descriptor.id == node)
    }

    #[must_use]
    pub fn record_by_path(
        self,
        path: AmlResolvedNamePath,
    ) -> Option<&'records AmlNamespaceLoadRecord> {
        self.records
            .iter()
            .find(|record| record.descriptor.path == path)
    }

    pub fn resolve_lookup_path(
        self,
        current_scope_path: AmlResolvedNamePath,
        encoded: AmlEncodedNameString<'_>,
    ) -> AmlResult<AmlResolvedNamePath> {
        match encoded.anchor {
            AmlNameAnchor::Root | AmlNameAnchor::ParentPrefix => {
                current_scope_path.resolve(encoded)
            }
            AmlNameAnchor::Local => {
                let mut scope = current_scope_path;
                loop {
                    let candidate = scope.resolve(encoded)?;
                    if self.record_by_path(candidate).is_some() {
                        return Ok(candidate);
                    }
                    match scope.parent() {
                        Some(parent) => scope = parent,
                        None => return Ok(candidate),
                    }
                }
            }
        }
    }

    #[must_use]
    pub fn code_bytes(self, location: AmlCodeLocation) -> Option<&'blocks [u8]> {
        let block = self.blocks.block(location.block_index)?;
        let start = usize::try_from(location.span.offset).ok()?;
        let end = start.checked_add(usize::try_from(location.span.length).ok()?)?;
        block.bytes.get(start..end)
    }
}
