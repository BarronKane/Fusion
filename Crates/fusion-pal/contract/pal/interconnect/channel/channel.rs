//! Universal channel transport contract.

mod caps;
mod error;
mod unsupported;

pub use caps::*;
use crate::contract::pal::interconnect::protocol::{
    Protocol,
    ProtocolDescriptor,
};
use crate::contract::pal::interconnect::transport::{
    TransportAttachmentControl,
    TransportSupport,
};
pub use error::*;
pub use unsupported::*;

/// Active channel mode for the first universal channel transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChannelMode {
    SingleProducerSingleConsumer,
    SingleProducerMultiConsumer,
}

/// One-way role surfaced by one channel endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChannelRole {
    Read,
    Write,
}

/// Full support surface for one channel transport instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChannelSupport {
    /// Capability flags honestly surfaced by the channel.
    pub caps: ChannelCaps,
    /// Native, emulated, or unsupported implementation category.
    pub implementation: ChannelImplementationKind,
    /// Current channel mode.
    pub mode: ChannelMode,
    /// Current producer count.
    pub producer_count: usize,
    /// Current consumer count.
    pub consumer_count: usize,
    /// Underlying transport support surface.
    pub transport: TransportSupport,
    /// Static protocol descriptor carried by the channel.
    pub protocol: ProtocolDescriptor,
}

/// Base contract for one protocol-anchored unidirectional channel instance.
///
/// Fusion channels are one-way only. Request/reply or duplex interactions are modeled as paired
/// channels rather than one bidirectional object pretending to be simpler than it is.
pub trait ChannelBase: TransportAttachmentControl {
    /// Protocol carried by this channel.
    type Protocol: Protocol;

    /// Returns the truthful support surface for this channel instance.
    fn channel_support(&self) -> ChannelSupport;
}

/// Write-side channel contract.
pub trait ChannelSend: ChannelBase {
    /// Sends one message through the channel.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the producer attachment is invalid, the buffer is full, or
    /// the channel cannot currently accept the message.
    fn try_send(
        &self,
        producer: Self::ProducerAttachment,
        message: <Self::Protocol as Protocol>::Message,
    ) -> Result<(), ChannelError>;
}

/// Read-side channel contract.
pub trait ChannelReceive: ChannelBase {
    /// Receives one message from the channel when available.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the consumer attachment is invalid or the channel cannot
    /// surface its next message honestly.
    fn try_receive(
        &self,
        consumer: Self::ConsumerAttachment,
    ) -> Result<Option<<Self::Protocol as Protocol>::Message>, ChannelError>;
}
