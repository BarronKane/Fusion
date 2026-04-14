//! AML method descriptors and lifecycle classification.

use crate::aml::{
    AmlCodeLocation,
    AmlMethodSerialization,
    AmlNamespaceNodeId,
};

/// Coarse lifecycle lane for one AML method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AmlMethodKind {
    Ordinary,
    Initialize,
    Status,
    RegionAvailability,
    NotificationQuery,
    EventHandler,
}

/// Stable descriptor for one loaded AML method object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlMethodDescriptor {
    pub node: AmlNamespaceNodeId,
    pub arg_count: u8,
    pub serialization: AmlMethodSerialization,
    pub sync_level: u8,
    pub kind: AmlMethodKind,
    pub body: AmlCodeLocation,
}
