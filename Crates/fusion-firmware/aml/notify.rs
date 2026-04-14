//! AML notification vocabulary.

use crate::aml::AmlNamespaceNodeId;

/// One AML notification event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlNotifyEvent {
    pub source: AmlNamespaceNodeId,
    pub value: u8,
}
