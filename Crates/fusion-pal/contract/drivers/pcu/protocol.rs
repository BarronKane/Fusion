//! PCU executor control-plane protocol vocabulary.
//!
//! These protocols are intentionally narrow:
//! - metadata flows out of one executor on one read channel
//! - submissions flow into one executor on one write channel
//! - status/completion flows out of one executor on one read channel
//!
//! Bulk payload does **not** belong in these messages. The control plane carries handles and
//! envelopes; the real data plane remains in binding-backed resources and port-backed edges.

use crate::contract::pal::interconnect::protocol::{
    Protocol,
    ProtocolBootstrapKind,
    ProtocolCaps,
    ProtocolDebugView,
    ProtocolDescriptor,
    ProtocolId,
    ProtocolImplementationKind,
    ProtocolTransportRequirements,
    ProtocolVersion,
};
use crate::contract::pal::interconnect::transport::{
    TransportDirection,
    TransportFraming,
};
use super::{
    PcuErrorKind,
    PcuExecutorDescriptor,
    PcuExecutorId,
    PcuInvocationShape,
    PcuKernelId,
};

/// Opaque submission identifier assigned by the submitter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuSubmissionId(pub u64);

/// Opaque handle naming one side-car attachment table or resource bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuAttachmentTableHandle(pub u64);

/// Opaque handle naming one side-car port-edge table or I/O-edge bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuPortTableHandle(pub u64);

/// Opaque handle naming one side-car runtime-parameter table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuParameterTableHandle(pub u64);

/// Metadata snapshot/event for one surfaced executor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuExecutorMetadataMessage {
    Advertised(PcuExecutorDescriptor),
    Withdrawn(PcuExecutorId),
}

/// Submission request sent to one executor write channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuSubmissionRequest {
    Submit {
        submission: PcuSubmissionId,
        kernel: PcuKernelId,
        invocation: PcuInvocationShape,
        binding_table: PcuAttachmentTableHandle,
        port_table: PcuPortTableHandle,
        parameter_table: PcuParameterTableHandle,
    },
    Cancel {
        submission: PcuSubmissionId,
    },
}

/// Submission status and completion events emitted by one executor read channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuSubmissionStatusMessage {
    Accepted {
        submission: PcuSubmissionId,
        executor: PcuExecutorId,
    },
    Rejected {
        submission: PcuSubmissionId,
        reason: PcuErrorKind,
    },
    Running {
        submission: PcuSubmissionId,
    },
    Completed {
        submission: PcuSubmissionId,
    },
    Failed {
        submission: PcuSubmissionId,
        reason: PcuErrorKind,
    },
    Cancelled {
        submission: PcuSubmissionId,
    },
}

/// Metadata/read protocol for surfaced PCU executors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuExecutorMetadataProtocol;

impl Protocol for PcuExecutorMetadataProtocol {
    type Message = PcuExecutorMetadataMessage;

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
        id: ProtocolId(0x4655_5349_4f4e_5043_555f_4d45_5441_0001),
        version: ProtocolVersion::new(1, 0, 0),
        caps: ProtocolCaps::empty(),
        bootstrap: ProtocolBootstrapKind::Immediate,
        debug_view: ProtocolDebugView::Structured,
        transport: ProtocolTransportRequirements {
            direction: TransportDirection::Unidirectional,
            framing: TransportFraming::Message,
            requires_ordering: true,
            requires_reliability: true,
            cross_courier_compatible: true,
            cross_domain_compatible: true,
        },
        implementation: ProtocolImplementationKind::Native,
    };
}

/// Submission/write protocol for sending work to one executor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuSubmissionWriteProtocol;

impl Protocol for PcuSubmissionWriteProtocol {
    type Message = PcuSubmissionRequest;

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
        id: ProtocolId(0x4655_5349_4f4e_5043_555f_5355_424d_0001),
        version: ProtocolVersion::new(1, 0, 0),
        caps: ProtocolCaps::empty(),
        bootstrap: ProtocolBootstrapKind::Immediate,
        debug_view: ProtocolDebugView::Structured,
        transport: ProtocolTransportRequirements {
            direction: TransportDirection::Unidirectional,
            framing: TransportFraming::Message,
            requires_ordering: true,
            requires_reliability: true,
            cross_courier_compatible: true,
            cross_domain_compatible: true,
        },
        implementation: ProtocolImplementationKind::Native,
    };
}

/// Status/read protocol for observing one executor's submission outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuSubmissionStatusProtocol;

impl Protocol for PcuSubmissionStatusProtocol {
    type Message = PcuSubmissionStatusMessage;

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
        id: ProtocolId(0x4655_5349_4f4e_5043_555f_5354_4154_0001),
        version: ProtocolVersion::new(1, 0, 0),
        caps: ProtocolCaps::empty(),
        bootstrap: ProtocolBootstrapKind::Immediate,
        debug_view: ProtocolDebugView::Structured,
        transport: ProtocolTransportRequirements {
            direction: TransportDirection::Unidirectional,
            framing: TransportFraming::Message,
            requires_ordering: true,
            requires_reliability: true,
            cross_courier_compatible: true,
            cross_domain_compatible: true,
        },
        implementation: ProtocolImplementationKind::Native,
    };
}
