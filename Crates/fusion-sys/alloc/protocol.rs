//! Allocator-domain audit protocol vocabulary.
//!
//! This first allocator protocol surface is intentionally narrow and honest:
//! - domain metadata is advertised on one read channel
//! - audit/stat requests flow in on one write channel
//! - replies flow out on one read channel
//!
//! Direct allocator APIs remain first-class. These protocols exist so allocator-domain truth can
//! ride protocol/transport/channel composition when the caller actually needs that boundary.

use crate::protocol::{
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
use crate::transport::{
    TransportDirection,
    TransportFraming,
};
use super::{
    AllocErrorKind,
    AllocatorDomainAudit,
    AllocatorDomainId,
    AllocatorDomainInfo,
    MemoryPoolExtentInfo,
    MemoryPoolMemberInfo,
    MemoryPoolStats,
};

/// Metadata snapshot/event for one surfaced allocator domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AllocatorDomainMetadataMessage {
    Advertised(AllocatorDomainInfo),
    Withdrawn(AllocatorDomainId),
}

/// Control request sent to one allocator-domain audit service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AllocatorControlRequest {
    ReadDomainAudit { domain: AllocatorDomainId },
    ReadDomainPoolStats { domain: AllocatorDomainId },
    ReadDomainPoolMembers { domain: AllocatorDomainId },
    ReadDomainPoolExtents { domain: AllocatorDomainId },
    RepublishDomains,
}

/// Control/status message emitted by one allocator-domain audit service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AllocatorControlStatusMessage {
    DomainAudit {
        domain: AllocatorDomainId,
        audit: AllocatorDomainAudit,
    },
    DomainPoolStats {
        domain: AllocatorDomainId,
        stats: Option<MemoryPoolStats>,
    },
    DomainPoolMember {
        domain: AllocatorDomainId,
        member: MemoryPoolMemberInfo,
    },
    DomainPoolMembersComplete {
        domain: AllocatorDomainId,
    },
    DomainPoolExtent {
        domain: AllocatorDomainId,
        extent: MemoryPoolExtentInfo,
    },
    DomainPoolExtentsComplete {
        domain: AllocatorDomainId,
    },
    MetadataRepublishScheduled,
    Rejected {
        domain: Option<AllocatorDomainId>,
        reason: AllocErrorKind,
    },
}

/// Metadata/read protocol for surfaced allocator domains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AllocatorDomainMetadataProtocol;

impl Protocol for AllocatorDomainMetadataProtocol {
    type Message = AllocatorDomainMetadataMessage;

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
        id: ProtocolId(0x4655_5349_4f4e_414c_4c4f_435f_4d44_0001),
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

/// Control/write protocol for allocator-domain audit requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AllocatorControlWriteProtocol;

impl Protocol for AllocatorControlWriteProtocol {
    type Message = AllocatorControlRequest;

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
        id: ProtocolId(0x4655_5349_4f4e_414c_4c4f_435f_4354_0001),
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

/// Status/read protocol for allocator-domain audit replies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AllocatorControlStatusProtocol;

impl Protocol for AllocatorControlStatusProtocol {
    type Message = AllocatorControlStatusMessage;

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
        id: ProtocolId(0x4655_5349_4f4e_414c_4c4f_435f_5354_0001),
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
