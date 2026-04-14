//! AML execution-context vocabulary.

use crate::aml::{
    AmlIntegerWidth,
    AmlNamespaceNodeId,
    AmlValue,
};

/// Coarse AML execution phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AmlExecutionPhase {
    LoadTime,
    Initialization,
    Runtime,
    Notification,
}

/// Borrowed AML invocation context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmlExecutionContext<'a> {
    pub integer_width: AmlIntegerWidth,
    pub phase: AmlExecutionPhase,
    pub current_scope: Option<AmlNamespaceNodeId>,
    pub current_method: Option<AmlNamespaceNodeId>,
    pub args: &'a [AmlValue<'a>],
    pub locals: &'a [Option<AmlValue<'a>>],
    pub recursion_depth: u16,
}
