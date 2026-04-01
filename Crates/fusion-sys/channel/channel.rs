//! fusion-sys channel wrappers and local fixed-capacity channel demonstration.

pub use fusion_pal::sys::channel::*;

use core::array;
use core::marker::PhantomData;

use crate::protocol::Protocol;
use crate::sync::{Mutex, SyncError, SyncErrorKind};
use crate::transport::{
    TransportAccessRequirement,
    TransportAttachmentControl,
    TransportAttachmentLaw,
    TransportAttachmentModel,
    TransportAttachmentRequest,
    TransportAttachmentScope,
    TransportBackpressure,
    TransportBase,
    TransportCaps,
    TransportDirection,
    TransportError,
    TransportFraming,
    TransportImplementationKind,
    TransportOrdering,
    TransportReliability,
    TransportSupport,
    TransportTopology,
    TransportWakeModel,
};

/// Local fixed-capacity channel transport used to prove the first channel contracts end to end.
///
/// This first implementation is intentionally narrow:
/// - unidirectional
/// - message framed
/// - reliable
/// - single producer
/// - SPSC by default
/// - promotes to SPMC when a second consumer attaches
/// - destructive queue semantics in SPMC mode
pub struct LocalChannel<P: Protocol, const CAPACITY: usize, const MAX_CONSUMERS: usize = 8> {
    state: Mutex<LocalChannelState<P::Message, CAPACITY, MAX_CONSUMERS>>,
    _protocol: PhantomData<P>,
}

struct LocalChannelState<T, const CAPACITY: usize, const MAX_CONSUMERS: usize> {
    buffer: [Option<T>; CAPACITY],
    head: usize,
    tail: usize,
    len: usize,
    attachment_law: TransportAttachmentLaw,
    next_attachment: usize,
    producer: Option<usize>,
    consumers: [Option<usize>; MAX_CONSUMERS],
}

impl<T, const CAPACITY: usize, const MAX_CONSUMERS: usize>
    LocalChannelState<T, CAPACITY, MAX_CONSUMERS>
{
    fn new(attachment_law: TransportAttachmentLaw) -> Self {
        Self {
            buffer: array::from_fn(|_| None),
            head: 0,
            tail: 0,
            len: 0,
            attachment_law,
            next_attachment: 1,
            producer: None,
            consumers: array::from_fn(|_| None),
        }
    }

    fn consumer_count(&self) -> usize {
        self.consumers
            .iter()
            .filter(|token| token.is_some())
            .count()
    }

    fn mode(&self) -> ChannelMode {
        if matches!(
            self.attachment_law,
            TransportAttachmentLaw::PromotableSpmc | TransportAttachmentLaw::SharedSpmc
        ) && self.consumer_count() > 1
        {
            ChannelMode::SingleProducerMultiConsumer
        } else {
            ChannelMode::SingleProducerSingleConsumer
        }
    }
}

impl<P: Protocol, const CAPACITY: usize, const MAX_CONSUMERS: usize>
    LocalChannel<P, CAPACITY, MAX_CONSUMERS>
{
    /// Creates a new local channel when the protocol is compatible with the local channel
    /// transport characteristics.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the protocol requires a transport shape this local channel
    /// cannot satisfy.
    pub fn new() -> Result<Self, ChannelError> {
        Self::new_with_attachment_law(TransportAttachmentLaw::PromotableSpmc)
    }

    /// Creates a new local channel with one explicit attachment law.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the protocol requires a transport shape this local channel
    /// cannot satisfy.
    pub fn new_with_attachment_law(
        attachment_law: TransportAttachmentLaw,
    ) -> Result<Self, ChannelError> {
        P::validate_transport(Self::transport_support(
            ChannelMode::SingleProducerSingleConsumer,
            attachment_law,
        ))
        .map_err(ChannelError::from)?;
        Ok(Self {
            state: Mutex::new(LocalChannelState::new(attachment_law)),
            _protocol: PhantomData,
        })
    }

    fn transport_support(
        mode: ChannelMode,
        attachment_law: TransportAttachmentLaw,
    ) -> TransportSupport {
        TransportSupport {
            caps: {
                let mut caps = TransportCaps::ATTACH_PRODUCER
                    | TransportCaps::ATTACH_CONSUMER
                    | TransportCaps::DETACH_PRODUCER
                    | TransportCaps::DETACH_CONSUMER
                    | TransportCaps::CROSS_COURIER_ATTACH
                    | TransportCaps::BUFFERED;
                if matches!(attachment_law, TransportAttachmentLaw::PromotableSpmc) {
                    caps |= TransportCaps::TOPOLOGY_PROMOTION;
                }
                caps
            },
            implementation: TransportImplementationKind::Native,
            direction: TransportDirection::Unidirectional,
            topology: match mode {
                ChannelMode::SingleProducerSingleConsumer => {
                    TransportTopology::SingleProducerSingleConsumer
                }
                ChannelMode::SingleProducerMultiConsumer => {
                    TransportTopology::SingleProducerMultiConsumer
                }
            },
            framing: TransportFraming::Message,
            ordering: TransportOrdering::Preserved,
            reliability: TransportReliability::Reliable,
            backpressure: TransportBackpressure::RejectWhenFull,
            attachment: TransportAttachmentModel::ScopedHandles,
            attachment_law,
            wake: TransportWakeModel::ExplicitPoll,
            same_courier_attach: TransportAccessRequirement::Available,
            cross_courier_attach: TransportAccessRequirement::Available,
            cross_domain_attach: TransportAccessRequirement::Unsupported,
        }
    }

    fn validate_attach_request(request: TransportAttachmentRequest) -> Result<(), TransportError> {
        match request.scope {
            TransportAttachmentScope::SameCourier => Ok(()),
            TransportAttachmentScope::CrossCourier => Ok(()),
            TransportAttachmentScope::CrossDomain => Err(TransportError::unsupported()),
        }
    }

    #[cfg(feature = "debug-insights")]
    pub(crate) fn clear_pending_messages(&self) -> Result<usize, ChannelError> {
        let mut state = self
            .state
            .try_lock()
            .map_err(channel_error_from_sync)?
            .ok_or_else(ChannelError::busy)?;
        let dropped = state.len;
        if dropped == 0 {
            return Ok(0);
        }

        for slot in &mut state.buffer {
            *slot = None;
        }
        state.head = 0;
        state.tail = 0;
        state.len = 0;
        Ok(dropped)
    }
}

impl<P: Protocol, const CAPACITY: usize, const MAX_CONSUMERS: usize> TransportBase
    for LocalChannel<P, CAPACITY, MAX_CONSUMERS>
{
    fn support(&self) -> TransportSupport {
        let state = self
            .state
            .lock()
            .expect("local channel state lock should acquire for support");
        Self::transport_support(state.mode(), state.attachment_law)
    }

    fn active_topology(&self) -> TransportTopology {
        self.support().topology
    }

    fn producer_count(&self) -> usize {
        usize::from(
            self.state
                .lock()
                .expect("local channel state lock should acquire for producer count")
                .producer
                .is_some(),
        )
    }

    fn consumer_count(&self) -> usize {
        self.state
            .lock()
            .expect("local channel state lock should acquire for consumer count")
            .consumer_count()
    }
}

impl<P: Protocol, const CAPACITY: usize, const MAX_CONSUMERS: usize> TransportAttachmentControl
    for LocalChannel<P, CAPACITY, MAX_CONSUMERS>
{
    type ProducerAttachment = usize;
    type ConsumerAttachment = usize;

    fn attach_producer(
        &self,
        request: TransportAttachmentRequest,
    ) -> Result<Self::ProducerAttachment, TransportError> {
        Self::validate_attach_request(request)?;
        let mut state = self
            .state
            .try_lock()
            .map_err(transport_error_from_sync)?
            .ok_or_else(TransportError::busy)?;
        if let Some(requested_law) = request.requested_law {
            if requested_law != state.attachment_law {
                return Err(TransportError::permission_denied());
            }
        }
        if state.producer.is_some() {
            return Err(TransportError::busy());
        }
        let token = state.next_attachment;
        state.next_attachment += 1;
        state.producer = Some(token);
        Ok(token)
    }

    fn attach_consumer(
        &self,
        request: TransportAttachmentRequest,
    ) -> Result<Self::ConsumerAttachment, TransportError> {
        Self::validate_attach_request(request)?;
        let mut state = self
            .state
            .try_lock()
            .map_err(transport_error_from_sync)?
            .ok_or_else(TransportError::busy)?;
        if let Some(requested_law) = request.requested_law {
            if requested_law != state.attachment_law {
                return Err(TransportError::permission_denied());
            }
        }
        if matches!(state.attachment_law, TransportAttachmentLaw::ExclusiveSpsc)
            && state.consumer_count() >= 1
        {
            return Err(TransportError::state_conflict());
        }
        let Some(slot_index) = state.consumers.iter().position(|slot| slot.is_none()) else {
            return Err(TransportError::resource_exhausted());
        };
        let token = state.next_attachment;
        state.next_attachment += 1;
        state.consumers[slot_index] = Some(token);
        Ok(token)
    }

    fn detach_producer(&self, attachment: Self::ProducerAttachment) -> Result<(), TransportError> {
        let mut state = self
            .state
            .try_lock()
            .map_err(transport_error_from_sync)?
            .ok_or_else(TransportError::busy)?;
        match state.producer {
            Some(token) if token == attachment => {
                state.producer = None;
                Ok(())
            }
            _ => Err(TransportError::not_attached()),
        }
    }

    fn detach_consumer(&self, attachment: Self::ConsumerAttachment) -> Result<(), TransportError> {
        let mut state = self
            .state
            .try_lock()
            .map_err(transport_error_from_sync)?
            .ok_or_else(TransportError::busy)?;
        let Some(slot) = state
            .consumers
            .iter_mut()
            .find(|slot| slot.is_some_and(|token| token == attachment))
        else {
            return Err(TransportError::not_attached());
        };
        *slot = None;
        Ok(())
    }
}

impl<P: Protocol, const CAPACITY: usize, const MAX_CONSUMERS: usize> ChannelBase
    for LocalChannel<P, CAPACITY, MAX_CONSUMERS>
{
    type Protocol = P;

    fn channel_support(&self) -> ChannelSupport {
        let state = self
            .state
            .lock()
            .expect("local channel state lock should acquire for support");
        let mode = state.mode();
        ChannelSupport {
            caps: {
                let mut caps = ChannelCaps::WRITE
                    | ChannelCaps::READ
                    | ChannelCaps::BUFFERED
                    | ChannelCaps::CLAIM_GATED_CROSS_COURIER;
                if matches!(state.attachment_law, TransportAttachmentLaw::PromotableSpmc) {
                    caps |= ChannelCaps::MODE_PROMOTION;
                }
                caps
            },
            implementation: ChannelImplementationKind::Native,
            mode,
            producer_count: usize::from(state.producer.is_some()),
            consumer_count: state.consumer_count(),
            transport: Self::transport_support(mode, state.attachment_law),
            protocol: P::DESCRIPTOR,
        }
    }
}

impl<P: Protocol, const CAPACITY: usize, const MAX_CONSUMERS: usize> ChannelSend
    for LocalChannel<P, CAPACITY, MAX_CONSUMERS>
{
    fn try_send(
        &self,
        producer: Self::ProducerAttachment,
        message: <Self::Protocol as Protocol>::Message,
    ) -> Result<(), ChannelError> {
        let mut state = self
            .state
            .try_lock()
            .map_err(channel_error_from_sync)?
            .ok_or_else(ChannelError::busy)?;
        if state.producer != Some(producer) {
            return Err(ChannelError::transport_denied());
        }
        if state.len == CAPACITY {
            return Err(ChannelError::busy());
        }
        let tail = state.tail;
        state.buffer[tail] = Some(message);
        state.tail = (state.tail + 1) % CAPACITY;
        state.len += 1;
        Ok(())
    }
}

impl<P: Protocol, const CAPACITY: usize, const MAX_CONSUMERS: usize> ChannelReceive
    for LocalChannel<P, CAPACITY, MAX_CONSUMERS>
{
    fn try_receive(
        &self,
        consumer: Self::ConsumerAttachment,
    ) -> Result<Option<<Self::Protocol as Protocol>::Message>, ChannelError> {
        let mut state = self
            .state
            .try_lock()
            .map_err(channel_error_from_sync)?
            .ok_or_else(ChannelError::busy)?;
        if !state
            .consumers
            .iter()
            .any(|slot| slot.is_some_and(|token| token == consumer))
        {
            return Err(ChannelError::transport_denied());
        }
        if state.len == 0 {
            return Ok(None);
        }
        let head = state.head;
        let message = state.buffer[head].take();
        state.head = (state.head + 1) % CAPACITY;
        state.len -= 1;
        Ok(message)
    }
}

const fn channel_error_from_sync(error: SyncError) -> ChannelError {
    match error.kind {
        SyncErrorKind::Unsupported => ChannelError::unsupported(),
        SyncErrorKind::Invalid | SyncErrorKind::Overflow => ChannelError::invalid(),
        SyncErrorKind::Busy => ChannelError::busy(),
        SyncErrorKind::PermissionDenied => ChannelError::permission_denied(),
        SyncErrorKind::Platform(code) => ChannelError::platform(code),
    }
}

const fn transport_error_from_sync(error: SyncError) -> TransportError {
    match error.kind {
        SyncErrorKind::Unsupported => TransportError::unsupported(),
        SyncErrorKind::Invalid | SyncErrorKind::Overflow => TransportError::invalid(),
        SyncErrorKind::Busy => TransportError::busy(),
        SyncErrorKind::PermissionDenied => TransportError::permission_denied(),
        SyncErrorKind::Platform(code) => TransportError::platform(code),
    }
}

#[cfg(all(test, feature = "std", not(target_os = "none")))]
mod tests {
    use super::*;
    use crate::protocol::{
        Protocol,
        ProtocolBootstrapKind,
        ProtocolCaps,
        ProtocolDebugView,
        ProtocolDescriptor,
        ProtocolId,
        ProtocolTransportRequirements,
        ProtocolVersion,
    };
    use crate::transport::TransportErrorKind;
    use std::sync::Arc;
    use std::thread;

    struct LocalWordProtocol;

    impl Protocol for LocalWordProtocol {
        type Message = u32;

        const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
            id: ProtocolId(0x4c4f_4341_4c5f_574f_5244),
            version: ProtocolVersion::new(1, 0, 0),
            caps: ProtocolCaps::VERSIONED,
            bootstrap: ProtocolBootstrapKind::Immediate,
            debug_view: ProtocolDebugView::Structured,
            transport: ProtocolTransportRequirements {
                direction: TransportDirection::Unidirectional,
                framing: TransportFraming::Message,
                requires_ordering: true,
                requires_reliability: true,
                cross_courier_compatible: true,
                cross_domain_compatible: false,
            },
            implementation: crate::protocol::ProtocolImplementationKind::Native,
        };
    }

    struct StreamOnlyProtocol;

    impl Protocol for StreamOnlyProtocol {
        type Message = u8;

        const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
            id: ProtocolId(7),
            version: ProtocolVersion::new(1, 0, 0),
            caps: ProtocolCaps::VERSIONED,
            bootstrap: ProtocolBootstrapKind::Immediate,
            debug_view: ProtocolDebugView::None,
            transport: ProtocolTransportRequirements {
                direction: TransportDirection::Unidirectional,
                framing: TransportFraming::Stream,
                requires_ordering: true,
                requires_reliability: true,
                cross_courier_compatible: false,
                cross_domain_compatible: false,
            },
            implementation: crate::protocol::ProtocolImplementationKind::Native,
        };
    }

    #[test]
    fn local_channel_spsc_send_receive_round_trip() {
        let channel = LocalChannel::<LocalWordProtocol, 4>::new().expect("channel should build");
        let producer = channel
            .attach_producer(TransportAttachmentRequest::same_courier())
            .expect("producer should attach");
        let consumer = channel
            .attach_consumer(TransportAttachmentRequest::same_courier())
            .expect("consumer should attach");

        channel
            .try_send(producer, 0xC0DE_CAFE)
            .expect("send should succeed");
        assert_eq!(
            channel
                .try_receive(consumer)
                .expect("receive should succeed"),
            Some(0xC0DE_CAFE)
        );
        assert_eq!(
            channel
                .try_receive(consumer)
                .expect("empty read should succeed"),
            None
        );
        assert_eq!(
            channel.channel_support().mode,
            ChannelMode::SingleProducerSingleConsumer
        );
    }

    #[test]
    fn local_channel_promotes_to_spmc_when_second_consumer_attaches() {
        let channel = LocalChannel::<LocalWordProtocol, 4>::new().expect("channel should build");
        channel
            .attach_producer(TransportAttachmentRequest::same_courier())
            .expect("producer should attach");
        let first = channel
            .attach_consumer(TransportAttachmentRequest::same_courier())
            .expect("first consumer should attach");
        assert_eq!(
            channel.channel_support().mode,
            ChannelMode::SingleProducerSingleConsumer
        );

        let second = channel
            .attach_consumer(TransportAttachmentRequest::same_courier())
            .expect("second consumer should attach");
        assert_eq!(
            channel.channel_support().mode,
            ChannelMode::SingleProducerMultiConsumer
        );

        channel
            .detach_consumer(first)
            .expect("first consumer should detach");
        channel
            .detach_consumer(second)
            .expect("second consumer should detach");
        assert_eq!(
            channel.channel_support().mode,
            ChannelMode::SingleProducerSingleConsumer
        );
    }

    #[test]
    fn broader_attachment_path_exists_without_committed_claims_model() {
        let channel = LocalChannel::<LocalWordProtocol, 4>::new().expect("channel should build");
        let allowed = channel.attach_consumer(TransportAttachmentRequest::cross_courier());
        assert!(allowed.is_ok());

        let denied = channel.attach_consumer(TransportAttachmentRequest::cross_domain());
        assert!(matches!(
            denied,
            Err(error) if error.kind() == TransportErrorKind::Unsupported
        ));
    }

    #[test]
    fn incompatible_protocol_is_rejected() {
        let result = LocalChannel::<StreamOnlyProtocol, 4>::new();
        assert!(matches!(
            result,
            Err(error) if error.kind() == ChannelErrorKind::ProtocolMismatch
        ));
    }

    #[test]
    fn local_channel_cross_thread_send_receive_round_trip() {
        let channel =
            Arc::new(LocalChannel::<LocalWordProtocol, 4>::new().expect("channel should build"));
        let producer = channel
            .attach_producer(TransportAttachmentRequest::cross_courier())
            .expect("producer should attach");
        let consumer = channel
            .attach_consumer(TransportAttachmentRequest::cross_courier())
            .expect("consumer should attach");
        let sender = Arc::clone(&channel);

        let thread = thread::spawn(move || {
            sender
                .try_send(producer, 0xABCD_1234)
                .expect("cross-thread send should succeed");
        });

        thread.join().expect("sender thread should finish");
        assert_eq!(
            channel
                .try_receive(consumer)
                .expect("cross-thread receive should succeed"),
            Some(0xABCD_1234)
        );
    }

    #[test]
    fn exclusive_spsc_channel_rejects_second_consumer() {
        let channel = LocalChannel::<LocalWordProtocol, 4>::new_with_attachment_law(
            TransportAttachmentLaw::ExclusiveSpsc,
        )
        .expect("channel should build");
        channel
            .attach_producer(
                TransportAttachmentRequest::same_courier()
                    .with_requested_law(TransportAttachmentLaw::ExclusiveSpsc),
            )
            .expect("producer should attach");
        channel
            .attach_consumer(
                TransportAttachmentRequest::same_courier()
                    .with_requested_law(TransportAttachmentLaw::ExclusiveSpsc),
            )
            .expect("first consumer should attach");

        let denied = channel.attach_consumer(
            TransportAttachmentRequest::same_courier()
                .with_requested_law(TransportAttachmentLaw::ExclusiveSpsc),
        );
        assert!(matches!(
            denied,
            Err(error) if error.kind() == TransportErrorKind::StateConflict
        ));
    }
}
