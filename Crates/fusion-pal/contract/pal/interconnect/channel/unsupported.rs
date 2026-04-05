//! Unsupported channel placeholder.

use crate::contract::pal::interconnect::protocol::{
    ProtocolContract,
    UnsupportedProtocol,
};
use crate::contract::pal::interconnect::transport::{
    TransportAttachmentControlContract,
    TransportAttachmentRequest,
    TransportBaseContract,
    TransportError,
    TransportSupport,
    TransportTopology,
    UnsupportedTransport,
};
use super::{
    ChannelBaseContract,
    ChannelError,
    ChannelReceiveContract,
    ChannelSendContract,
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

impl TransportBaseContract for UnsupportedChannel {
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

impl TransportAttachmentControlContract for UnsupportedChannel {
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

impl ChannelBaseContract for UnsupportedChannel {
    type ProtocolContract = UnsupportedProtocol;

    fn channel_support(&self) -> ChannelSupport {
        ChannelSupport {
            caps: super::ChannelCaps::empty(),
            implementation: super::ChannelImplementationKind::Unsupported,
            mode: super::ChannelMode::SingleProducerSingleConsumer,
            producer_count: 0,
            consumer_count: 0,
            transport: TransportSupport::unsupported(),
            protocol: <Self::ProtocolContract as ProtocolContract>::DESCRIPTOR,
        }
    }
}

impl ChannelSendContract for UnsupportedChannel {
    fn try_send(
        &self,
        _producer: Self::ProducerAttachment,
        _message: <Self::ProtocolContract as ProtocolContract>::Message,
    ) -> Result<(), ChannelError> {
        Err(ChannelError::unsupported())
    }
}

impl ChannelReceiveContract for UnsupportedChannel {
    fn try_receive(
        &self,
        _consumer: Self::ConsumerAttachment,
    ) -> Result<Option<<Self::ProtocolContract as ProtocolContract>::Message>, ChannelError> {
        Err(ChannelError::unsupported())
    }
}
