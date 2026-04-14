//! AML trace and source-location vocabulary.

use crate::aml::{
    AmlCodeLocation,
    AmlNamespaceNodeId,
};

/// Source location for one lowered or interpreted AML action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlSourceLocation {
    pub code: AmlCodeLocation,
}

/// Coarse AML trace event kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AmlTraceEventKind {
    MethodEnter,
    MethodReturn,
    RegionRead,
    RegionWrite,
    Notify,
}

/// One trace event emitted by the AML VM or lowering path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlTraceEvent {
    pub node: Option<AmlNamespaceNodeId>,
    pub location: Option<AmlSourceLocation>,
    pub kind: AmlTraceEventKind,
}
