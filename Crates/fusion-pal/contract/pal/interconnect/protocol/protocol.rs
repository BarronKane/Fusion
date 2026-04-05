//! Universal protocol contract vocabulary.

mod caps;
mod error;
mod unsupported;

pub use caps::*;
use crate::contract::pal::interconnect::transport::{
    TransportAccessRequirement,
    TransportDirection,
    TransportFraming,
    TransportOrdering,
    TransportReliability,
    TransportSupport,
};
pub use error::*;
pub use unsupported::*;

/// Stable protocol identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProtocolId(pub u128);

/// Stable protocol version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProtocolVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl ProtocolVersion {
    #[must_use]
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

/// Bootstrap strategy required by a protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProtocolBootstrapKind {
    /// No explicit bootstrap phase is required.
    Immediate,
    /// The protocol requires one negotiation/bootstrap exchange first.
    Negotiated,
}

/// Debug/introspection view surfaced by a protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProtocolDebugView {
    /// No debug/introspection view is defined.
    None,
    /// One structured debug/introspection representation exists.
    Structured,
    /// One plaintext adapter view exists.
    PlaintextAdapter,
}

/// Transport requirements declared by a protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProtocolTransportRequirements {
    /// Required direction model.
    pub direction: TransportDirection,
    /// Required framing model.
    pub framing: TransportFraming,
    /// Whether the protocol requires preserved ordering.
    pub requires_ordering: bool,
    /// Whether the protocol requires reliable delivery.
    pub requires_reliability: bool,
    /// Whether the protocol is meaningful across courier boundaries.
    pub cross_courier_compatible: bool,
    /// Whether the protocol is meaningful across domain boundaries.
    pub cross_domain_compatible: bool,
}

impl ProtocolTransportRequirements {
    /// Returns one minimal immediate message-framed local protocol profile.
    #[must_use]
    pub const fn message_local() -> Self {
        Self {
            direction: TransportDirection::Unidirectional,
            framing: TransportFraming::Message,
            requires_ordering: true,
            requires_reliability: true,
            cross_courier_compatible: false,
            cross_domain_compatible: false,
        }
    }
}

/// Full static descriptor for a protocol contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProtocolDescriptor {
    /// Stable protocol identifier.
    pub id: ProtocolId,
    /// Stable protocol version.
    pub version: ProtocolVersion,
    /// Capability flags honestly surfaced by the protocol contract.
    pub caps: ProtocolCaps,
    /// Bootstrap strategy.
    pub bootstrap: ProtocolBootstrapKind,
    /// Debug/introspection view.
    pub debug_view: ProtocolDebugView,
    /// Declared transport requirements.
    pub transport: ProtocolTransportRequirements,
    /// Native or unsupported protocol descriptor category.
    pub implementation: ProtocolImplementationKind,
}

impl ProtocolDescriptor {
    /// Returns one fully unsupported descriptor placeholder.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            id: ProtocolId(0),
            version: ProtocolVersion::new(0, 0, 0),
            caps: ProtocolCaps::empty(),
            bootstrap: ProtocolBootstrapKind::Immediate,
            debug_view: ProtocolDebugView::None,
            transport: ProtocolTransportRequirements::message_local(),
            implementation: ProtocolImplementationKind::Unsupported,
        }
    }
}

/// Static protocol contract.
pub trait ProtocolContract {
    /// Typed message payload carried by this protocol.
    type Message;

    /// Static protocol descriptor.
    const DESCRIPTOR: ProtocolDescriptor;

    /// Validates that one transport surface can honestly carry this protocol.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the transport cannot satisfy the declared protocol
    /// requirements.
    fn validate_transport(transport: TransportSupport) -> Result<(), ProtocolError> {
        if Self::DESCRIPTOR.implementation == ProtocolImplementationKind::Unsupported {
            return Err(ProtocolError::unsupported());
        }

        let requirements = Self::DESCRIPTOR.transport;

        if requirements.direction != transport.direction {
            return Err(ProtocolError::transport_mismatch());
        }
        if requirements.framing != transport.framing {
            return Err(ProtocolError::transport_mismatch());
        }
        if requirements.requires_ordering && transport.ordering != TransportOrdering::Preserved {
            return Err(ProtocolError::transport_mismatch());
        }
        if requirements.requires_reliability
            && transport.reliability != TransportReliability::Reliable
        {
            return Err(ProtocolError::transport_mismatch());
        }
        let _ = TransportAccessRequirement::Unsupported;
        let _ = requirements.cross_courier_compatible;
        let _ = requirements.cross_domain_compatible;

        Ok(())
    }
}
