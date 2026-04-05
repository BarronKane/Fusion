//! Unsupported transport placeholder.

use super::{
    TransportAttachmentControlContract,
    TransportAttachmentRequest,
    TransportBaseContract,
    TransportError,
    TransportSupport,
    TransportTopology,
};

/// Unsupported transport placeholder.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedTransport;

impl UnsupportedTransport {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl TransportBaseContract for UnsupportedTransport {
    fn support(&self) -> TransportSupport {
        TransportSupport::unsupported()
    }

    fn active_topology(&self) -> TransportTopology {
        TransportSupport::unsupported().topology
    }

    fn producer_count(&self) -> usize {
        0
    }

    fn consumer_count(&self) -> usize {
        0
    }
}

impl TransportAttachmentControlContract for UnsupportedTransport {
    type ProducerAttachment = ();
    type ConsumerAttachment = ();

    fn attach_producer(
        &self,
        _request: TransportAttachmentRequest,
    ) -> Result<Self::ProducerAttachment, TransportError> {
        Err(TransportError::unsupported())
    }

    fn attach_consumer(
        &self,
        _request: TransportAttachmentRequest,
    ) -> Result<Self::ConsumerAttachment, TransportError> {
        Err(TransportError::unsupported())
    }

    fn detach_producer(&self, _attachment: Self::ProducerAttachment) -> Result<(), TransportError> {
        Err(TransportError::unsupported())
    }

    fn detach_consumer(&self, _attachment: Self::ConsumerAttachment) -> Result<(), TransportError> {
        Err(TransportError::unsupported())
    }
}
