//! Universal transport-layer contract vocabulary.

mod caps;
mod error;
mod unsupported;

pub use caps::*;
pub use error::*;
pub use unsupported::*;

/// One transport direction model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransportDirection {
    /// One-way transport from producer to consumer.
    Unidirectional,
}

/// One active producer/consumer topology.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransportTopology {
    SingleProducerSingleConsumer,
    SingleProducerMultiConsumer,
    MultiProducerSingleConsumer,
    MultiProducerMultiConsumer,
}

/// One framing model surfaced by a transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransportFraming {
    /// Typed or opaque discrete messages.
    Message,
    /// Continuous ordered stream.
    Stream,
    /// Packetized transport with explicit packet boundaries.
    Packet,
}

/// One ordering guarantee surfaced by a transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransportOrdering {
    /// The transport preserves send order.
    Preserved,
    /// The transport may reorder.
    Unordered,
}

/// One reliability model surfaced by a transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransportReliability {
    /// Payloads are retained until successfully delivered or explicitly detached.
    Reliable,
    /// Payloads may be lost without strict recovery.
    BestEffort,
}

/// One backpressure model surfaced by a transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransportBackpressure {
    /// Producers must retry or yield when the transport is full.
    RejectWhenFull,
    /// Producers may yield and resume when capacity returns.
    YieldUntilSpace,
    /// Producers may block the caller until capacity returns.
    BlockUntilSpace,
}

/// One attachment lifecycle model surfaced by a transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransportAttachmentModel {
    /// Callers explicitly attach and later detach scoped handles or tokens.
    ScopedHandles,
    /// Attachments are fixed by construction and not dynamically managed.
    FixedEndpoints,
}

/// One wake/progress model surfaced by a transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransportWakeModel {
    /// Callers must poll explicitly.
    ExplicitPoll,
    /// The transport can surface readiness-style wakeups.
    Readiness,
    /// The transport can surface completion-style wakeups.
    Completion,
}

/// Attachment law declared by one transport implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransportAttachmentLaw {
    /// Exactly one producer and one consumer may attach for the lifetime of the relationship.
    ExclusiveSpsc,
    /// The transport starts as SPSC and may promote to SPMC if the implementation allows it.
    PromotableSpmc,
    /// The transport is intentionally shared and may accept multiple consumers immediately.
    SharedSpmc,
}

/// Scope in which one attachment is being requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransportAttachmentScope {
    /// The attachment stays within one courier.
    SameCourier,
    /// The attachment crosses courier boundaries inside one domain.
    CrossCourier,
    /// The attachment crosses domain boundaries.
    CrossDomain,
}

/// Effective attachment rule for one scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransportAccessRequirement {
    /// The scope is unsupported by this transport.
    Unsupported,
    /// The scope is available on this transport.
    Available,
}

/// Full attachment request for one transport endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TransportAttachmentRequest {
    /// Scope in which the attachment is requested.
    pub scope: TransportAttachmentScope,
    /// Preferred attachment law requested by the caller, when the endpoint cares to state one.
    pub requested_law: Option<TransportAttachmentLaw>,
}

impl TransportAttachmentRequest {
    /// Returns one same-courier attachment request.
    #[must_use]
    pub const fn same_courier() -> Self {
        Self {
            scope: TransportAttachmentScope::SameCourier,
            requested_law: None,
        }
    }

    /// Returns one cross-courier attachment request.
    #[must_use]
    pub const fn cross_courier() -> Self {
        Self {
            scope: TransportAttachmentScope::CrossCourier,
            requested_law: None,
        }
    }

    /// Returns one cross-domain attachment request.
    #[must_use]
    pub const fn cross_domain() -> Self {
        Self {
            scope: TransportAttachmentScope::CrossDomain,
            requested_law: None,
        }
    }

    /// Returns one attachment request explicitly naming one desired law.
    #[must_use]
    pub const fn with_requested_law(mut self, requested_law: TransportAttachmentLaw) -> Self {
        self.requested_law = Some(requested_law);
        self
    }
}

/// Full support surface for one transport implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TransportSupport {
    /// Capability flags honestly surfaced by the transport.
    pub caps: TransportCaps,
    /// Native, emulated, or unsupported implementation category.
    pub implementation: TransportImplementationKind,
    /// Active transport direction.
    pub direction: TransportDirection,
    /// Active transport topology.
    pub topology: TransportTopology,
    /// Active framing model.
    pub framing: TransportFraming,
    /// Active ordering model.
    pub ordering: TransportOrdering,
    /// Active reliability model.
    pub reliability: TransportReliability,
    /// Active backpressure model.
    pub backpressure: TransportBackpressure,
    /// Attachment lifecycle model.
    pub attachment: TransportAttachmentModel,
    /// Declared attachment law.
    pub attachment_law: TransportAttachmentLaw,
    /// Wake/progress model.
    pub wake: TransportWakeModel,
    /// Same-courier attachment rule.
    pub same_courier_attach: TransportAccessRequirement,
    /// Cross-courier attachment rule.
    pub cross_courier_attach: TransportAccessRequirement,
    /// Cross-domain attachment rule.
    pub cross_domain_attach: TransportAccessRequirement,
}

impl TransportSupport {
    /// Returns a fully unsupported transport surface.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            caps: TransportCaps::empty(),
            implementation: TransportImplementationKind::Unsupported,
            direction: TransportDirection::Unidirectional,
            topology: TransportTopology::SingleProducerSingleConsumer,
            framing: TransportFraming::Message,
            ordering: TransportOrdering::Unordered,
            reliability: TransportReliability::BestEffort,
            backpressure: TransportBackpressure::RejectWhenFull,
            attachment: TransportAttachmentModel::ScopedHandles,
            attachment_law: TransportAttachmentLaw::ExclusiveSpsc,
            wake: TransportWakeModel::ExplicitPoll,
            same_courier_attach: TransportAccessRequirement::Unsupported,
            cross_courier_attach: TransportAccessRequirement::Unsupported,
            cross_domain_attach: TransportAccessRequirement::Unsupported,
        }
    }
}

/// Base capability surface for a transport implementation.
pub trait TransportBase {
    /// Reports the truthful transport support surface.
    fn support(&self) -> TransportSupport;

    /// Returns the currently active topology.
    fn active_topology(&self) -> TransportTopology;

    /// Returns the number of attached producers.
    fn producer_count(&self) -> usize;

    /// Returns the number of attached consumers.
    fn consumer_count(&self) -> usize;
}

/// Dynamic attachment contract for one transport implementation.
pub trait TransportAttachmentControl: TransportBase {
    /// Producer attachment token or handle identifier.
    type ProducerAttachment: Copy + Eq;
    /// Consumer attachment token or handle identifier.
    type ConsumerAttachment: Copy + Eq;

    /// Attaches one producer.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the scope is unsupported or the transport cannot accept
    /// another producer.
    fn attach_producer(
        &self,
        request: TransportAttachmentRequest,
    ) -> Result<Self::ProducerAttachment, TransportError>;

    /// Attaches one consumer.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the scope is unsupported or the transport cannot accept
    /// another consumer.
    fn attach_consumer(
        &self,
        request: TransportAttachmentRequest,
    ) -> Result<Self::ConsumerAttachment, TransportError>;

    /// Detaches one producer.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the attachment token is unknown or already detached.
    fn detach_producer(&self, attachment: Self::ProducerAttachment) -> Result<(), TransportError>;

    /// Detaches one consumer.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the attachment token is unknown or already detached.
    fn detach_consumer(&self, attachment: Self::ConsumerAttachment) -> Result<(), TransportError>;
}
