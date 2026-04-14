//! AML lowering vocabulary for execution-model targeting.

use fusion_pcu::contract::ir::PcuIrKind;

use crate::aml::AmlNamespaceNodeId;

/// Coarse AML lowering lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AmlLoweringTargetKind {
    Interpret,
    Command,
    Signal,
    Transaction,
    Dispatch,
}

impl AmlLoweringTargetKind {
    #[must_use]
    pub const fn pcu_ir_kind(self) -> Option<PcuIrKind> {
        match self {
            Self::Interpret => None,
            Self::Command => Some(PcuIrKind::Command),
            Self::Signal => Some(PcuIrKind::Signal),
            Self::Transaction => Some(PcuIrKind::Transaction),
            Self::Dispatch => Some(PcuIrKind::Dispatch),
        }
    }
}

/// One AML lowering plan for a namespace method or handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlLoweringPlan {
    pub source: AmlNamespaceNodeId,
    pub target: AmlLoweringTargetKind,
}

/// Marker trait for lowering targets.
pub trait AmlLoweringTarget {
    fn lowering_target(&self) -> AmlLoweringTargetKind;
}
