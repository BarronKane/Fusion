//! Unsupported channel placeholder.

use crate::contract::pal::interconnect::protocol::{
    Protocol,
    UnsupportedProtocol,
};
use crate::contract::pal::interconnect::transport::{
    TransportAttachmentControl,
    TransportAttachmentRequest,
    TransportBase,
    TransportError,
    TransportSupport,
    TransportTopology,
    UnsupportedTransport,
};
use super::{
    ChannelBase,
    ChannelError,
    ChannelReceive,
    ChannelSend,
    ChannelSupport,
};

/// Unsupported channel placeholder.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedChannel;

impl UnsupportedChannel {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl TransportBase for UnsupportedChannel {
    fn support(&self) -> TransportSupport {
        UnsupportedTransport::new().support()
    }

    fn active_topology(&self) -> TransportTopology {
        UnsupportedTransport::new().active_topology()
    }

    fn producer_count(&self) -> usize {
        0
    }

    fn consumer_count(&self) -> usize {
        0
    }
}

impl TransportAttachmentControl for UnsupportedChannel {
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

impl ChannelBase for UnsupportedChannel {
    type Protocol = UnsupportedProtocol;

    fn channel_support(&self) -> ChannelSupport {
        ChannelSupport {
            caps: super::ChannelCaps::empty(),
            implementation: super::ChannelImplementationKind::Unsupported,
            mode: super::ChannelMode::SingleProducerSingleConsumer,
            producer_count: 0,
            consumer_count: 0,
            transport: TransportSupport::unsupported(),
            protocol: <Self::Protocol as Protocol>::DESCRIPTOR,
        }
    }
}

impl ChannelSend for UnsupportedChannel {
    fn try_send(
        &self,
        _producer: Self::ProducerAttachment,
        _message: <Self::Protocol as Protocol>::Message,
    ) -> Result<(), ChannelError> {
        Err(ChannelError::unsupported())
    }
}

impl ChannelReceive for UnsupportedChannel {
    fn try_receive(
        &self,
        _consumer: Self::ConsumerAttachment,
    ) -> Result<Option<<Self::Protocol as Protocol>::Message>, ChannelError> {
        Err(ChannelError::unsupported())
    }
}
