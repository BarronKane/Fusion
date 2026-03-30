//! fusion-sys channel-native debug/inspection surfaces.
//!
//! The front door is always present. When `debug-insights` is disabled, construction is rejected
//! honestly and the disabled implementation compiles down to near nothing in release builds.

mod timeline;

pub use fusion_pal::sys::insight::*;
pub use timeline::*;

use core::marker::PhantomData;
use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

#[cfg(feature = "debug-insights")]
use crate::channel::LocalChannel;
use crate::channel::{ChannelBase, ChannelError, ChannelReceive, ChannelSend, ChannelSupport};
#[cfg(not(feature = "debug-insights"))]
use crate::channel::{ChannelCaps, ChannelImplementationKind, ChannelMode};
use crate::protocol::Protocol;
use crate::transport::{
    TransportAttachmentControl,
    TransportAttachmentRequest,
    TransportBase,
    TransportError,
    TransportSupport,
    TransportTopology,
};

/// One local insight side channel.
///
/// This is a dedicated debug/inspection channel, not one overloaded application channel wearing a
/// sheep's skin. When the `debug-insights` feature is disabled, construction returns
/// `InsightError::not_enabled()` and the disabled implementation remains available only as a
/// zero-cost front door for the optimizer to erase.
pub struct LocalInsightChannel<P: Protocol, const CAPACITY: usize, const MAX_CONSUMERS: usize = 8> {
    class: InsightChannelClass,
    capture: InsightCaptureMode,
    #[cfg(feature = "debug-insights")]
    inner: LocalChannel<P, CAPACITY, MAX_CONSUMERS>,
    observation_count: AtomicUsize,
    observation_state: AtomicU8,
    observation_epoch: AtomicUsize,
    pending_transition: AtomicU8,
    _protocol: PhantomData<P>,
}

/// Whether one insight side channel currently has at least one attached observer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InsightObservationState {
    /// No consumers are attached, so new capture should remain dormant.
    Inactive,
    /// At least one consumer is attached, so new capture may proceed.
    Active,
}

/// One observer-lifecycle edge for an insight side channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InsightObservationTransition {
    /// The first consumer attached and observation became active.
    Activated,
    /// The last consumer detached and observation became inactive.
    Deactivated,
}

impl<P: Protocol, const CAPACITY: usize, const MAX_CONSUMERS: usize>
    LocalInsightChannel<P, CAPACITY, MAX_CONSUMERS>
{
    /// Returns the configured support surface for this insight channel class.
    #[must_use]
    pub const fn configured_support(
        class: InsightChannelClass,
        capture: InsightCaptureMode,
    ) -> InsightSupport {
        #[cfg(feature = "debug-insights")]
        {
            InsightSupport::available(class, capture)
        }
        #[cfg(not(feature = "debug-insights"))]
        {
            InsightSupport::disabled_by_feature(class, capture)
        }
    }

    /// Creates one local insight side channel.
    ///
    /// # Errors
    ///
    /// Returns `InsightError::not_enabled()` when `debug-insights` is disabled.
    pub fn new(
        class: InsightChannelClass,
        capture: InsightCaptureMode,
    ) -> Result<Self, InsightError> {
        #[cfg(feature = "debug-insights")]
        {
            let inner = LocalChannel::<P, CAPACITY, MAX_CONSUMERS>::new()?;
            Ok(Self {
                class,
                capture,
                inner,
                observation_count: AtomicUsize::new(0),
                observation_state: AtomicU8::new(observation_state_code(
                    InsightObservationState::Inactive,
                )),
                observation_epoch: AtomicUsize::new(0),
                pending_transition: AtomicU8::new(0),
                _protocol: PhantomData,
            })
        }
        #[cfg(not(feature = "debug-insights"))]
        {
            let _ = (class, capture);
            Err(InsightError::not_enabled())
        }
    }

    #[cfg(feature = "debug-insights")]
    fn activate_observation(&self) {
        self.observation_state.store(
            observation_state_code(InsightObservationState::Active),
            Ordering::Release,
        );
        self.observation_epoch.fetch_add(1, Ordering::AcqRel);
        self.pending_transition.store(
            observation_transition_code(Some(InsightObservationTransition::Activated)),
            Ordering::Release,
        );
    }

    #[cfg(feature = "debug-insights")]
    fn deactivate_observation(&self) {
        self.observation_state.store(
            observation_state_code(InsightObservationState::Inactive),
            Ordering::Release,
        );
        self.observation_epoch.fetch_add(1, Ordering::AcqRel);
        self.pending_transition.store(
            observation_transition_code(Some(InsightObservationTransition::Deactivated)),
            Ordering::Release,
        );
    }

    /// Returns the truthful insight support surface for this configured channel.
    #[must_use]
    pub const fn insight_support(&self) -> InsightSupport {
        Self::configured_support(self.class, self.capture)
    }

    /// Returns the configured insight class.
    #[must_use]
    pub const fn class(&self) -> InsightChannelClass {
        self.class
    }

    /// Returns the configured capture mode.
    #[must_use]
    pub const fn capture(&self) -> InsightCaptureMode {
        self.capture
    }

    /// Returns the current observer lifecycle state.
    #[must_use]
    pub fn observation_state(&self) -> InsightObservationState {
        decode_observation_state(self.observation_state.load(Ordering::Acquire))
    }

    /// Returns the current observer lifecycle epoch.
    ///
    /// The epoch increments only when the channel crosses `Inactive <-> Active`, so higher layers
    /// can latch capture sessions without mistaking every attach for a new trace.
    #[must_use]
    pub fn observation_epoch(&self) -> u64 {
        self.observation_epoch.load(Ordering::Acquire) as u64
    }

    /// Returns one pending observer lifecycle transition, if one has occurred since the last poll.
    #[must_use]
    pub fn take_observation_transition(&self) -> Option<InsightObservationTransition> {
        decode_observation_transition(self.pending_transition.swap(0, Ordering::AcqRel))
    }

    /// Returns `true` when at least one consumer is currently attached.
    #[must_use]
    pub fn is_observed(&self) -> bool {
        self.observation_count.load(Ordering::Acquire) != 0
    }

    /// Builds and sends one insight payload only when the channel is currently observed.
    ///
    /// Returns `Ok(false)` when no consumer is attached, so the caller can skip all expensive
    /// capture work in release builds with insight enabled but inactive.
    pub fn try_send_if_observed<F>(&self, producer: usize, build: F) -> Result<bool, ChannelError>
    where
        F: FnOnce() -> P::Message,
    {
        if !self.is_observed() {
            return Ok(false);
        }

        self.try_send(producer, build())?;
        Ok(true)
    }

    pub(crate) fn clear_pending_messages(&self) -> Result<usize, ChannelError> {
        #[cfg(feature = "debug-insights")]
        {
            self.inner.clear_pending_messages()
        }
        #[cfg(not(feature = "debug-insights"))]
        {
            Ok(0)
        }
    }
}

impl<P: Protocol, const CAPACITY: usize, const MAX_CONSUMERS: usize> TransportBase
    for LocalInsightChannel<P, CAPACITY, MAX_CONSUMERS>
{
    fn support(&self) -> TransportSupport {
        #[cfg(feature = "debug-insights")]
        {
            self.inner.support()
        }
        #[cfg(not(feature = "debug-insights"))]
        {
            TransportSupport::unsupported()
        }
    }

    fn active_topology(&self) -> TransportTopology {
        self.support().topology
    }

    fn producer_count(&self) -> usize {
        #[cfg(feature = "debug-insights")]
        {
            self.inner.producer_count()
        }
        #[cfg(not(feature = "debug-insights"))]
        {
            0
        }
    }

    fn consumer_count(&self) -> usize {
        #[cfg(feature = "debug-insights")]
        {
            self.inner.consumer_count()
        }
        #[cfg(not(feature = "debug-insights"))]
        {
            0
        }
    }
}

impl<P: Protocol, const CAPACITY: usize, const MAX_CONSUMERS: usize> TransportAttachmentControl
    for LocalInsightChannel<P, CAPACITY, MAX_CONSUMERS>
{
    type ProducerAttachment = usize;
    type ConsumerAttachment = usize;

    fn attach_producer(
        &self,
        request: TransportAttachmentRequest,
    ) -> Result<Self::ProducerAttachment, TransportError> {
        #[cfg(feature = "debug-insights")]
        {
            self.inner.attach_producer(request)
        }
        #[cfg(not(feature = "debug-insights"))]
        {
            let _ = request;
            Err(TransportError::unsupported())
        }
    }

    fn attach_consumer(
        &self,
        request: TransportAttachmentRequest,
    ) -> Result<Self::ConsumerAttachment, TransportError> {
        #[cfg(feature = "debug-insights")]
        {
            let attachment = self.inner.attach_consumer(request)?;
            if self.observation_count.fetch_add(1, Ordering::AcqRel) == 0 {
                self.activate_observation();
            }
            Ok(attachment)
        }
        #[cfg(not(feature = "debug-insights"))]
        {
            let _ = request;
            Err(TransportError::unsupported())
        }
    }

    fn detach_producer(&self, attachment: Self::ProducerAttachment) -> Result<(), TransportError> {
        #[cfg(feature = "debug-insights")]
        {
            self.inner.detach_producer(attachment)
        }
        #[cfg(not(feature = "debug-insights"))]
        {
            let _ = attachment;
            Err(TransportError::unsupported())
        }
    }

    fn detach_consumer(&self, attachment: Self::ConsumerAttachment) -> Result<(), TransportError> {
        #[cfg(feature = "debug-insights")]
        {
            self.inner.detach_consumer(attachment)?;
            let previous = self.observation_count.fetch_sub(1, Ordering::AcqRel);
            debug_assert!(previous != 0, "insight consumer count underflow");
            if previous == 1 {
                self.deactivate_observation();
            }
            Ok(())
        }
        #[cfg(not(feature = "debug-insights"))]
        {
            let _ = attachment;
            Err(TransportError::unsupported())
        }
    }
}

#[cfg(feature = "debug-insights")]
const fn observation_state_code(state: InsightObservationState) -> u8 {
    match state {
        InsightObservationState::Inactive => 0,
        InsightObservationState::Active => 1,
    }
}

const fn decode_observation_state(code: u8) -> InsightObservationState {
    match code {
        1 => InsightObservationState::Active,
        _ => InsightObservationState::Inactive,
    }
}

#[cfg(feature = "debug-insights")]
const fn observation_transition_code(transition: Option<InsightObservationTransition>) -> u8 {
    match transition {
        None => 0,
        Some(InsightObservationTransition::Activated) => 1,
        Some(InsightObservationTransition::Deactivated) => 2,
    }
}

const fn decode_observation_transition(code: u8) -> Option<InsightObservationTransition> {
    match code {
        1 => Some(InsightObservationTransition::Activated),
        2 => Some(InsightObservationTransition::Deactivated),
        _ => None,
    }
}

impl<P: Protocol, const CAPACITY: usize, const MAX_CONSUMERS: usize> ChannelBase
    for LocalInsightChannel<P, CAPACITY, MAX_CONSUMERS>
{
    type Protocol = P;

    fn channel_support(&self) -> ChannelSupport {
        #[cfg(feature = "debug-insights")]
        {
            self.inner.channel_support()
        }
        #[cfg(not(feature = "debug-insights"))]
        {
            ChannelSupport {
                caps: ChannelCaps::empty(),
                implementation: ChannelImplementationKind::Unsupported,
                mode: ChannelMode::SingleProducerSingleConsumer,
                producer_count: 0,
                consumer_count: 0,
                transport: TransportSupport::unsupported(),
                protocol: P::DESCRIPTOR,
            }
        }
    }
}

impl<P: Protocol, const CAPACITY: usize, const MAX_CONSUMERS: usize> ChannelSend
    for LocalInsightChannel<P, CAPACITY, MAX_CONSUMERS>
{
    fn try_send(
        &self,
        producer: Self::ProducerAttachment,
        message: <Self::Protocol as Protocol>::Message,
    ) -> Result<(), ChannelError> {
        #[cfg(feature = "debug-insights")]
        {
            self.inner.try_send(producer, message)
        }
        #[cfg(not(feature = "debug-insights"))]
        {
            let _ = (producer, message);
            Err(ChannelError::unsupported())
        }
    }
}

impl<P: Protocol, const CAPACITY: usize, const MAX_CONSUMERS: usize> ChannelReceive
    for LocalInsightChannel<P, CAPACITY, MAX_CONSUMERS>
{
    fn try_receive(
        &self,
        consumer: Self::ConsumerAttachment,
    ) -> Result<Option<<Self::Protocol as Protocol>::Message>, ChannelError> {
        #[cfg(feature = "debug-insights")]
        {
            self.inner.try_receive(consumer)
        }
        #[cfg(not(feature = "debug-insights"))]
        {
            let _ = consumer;
            Err(ChannelError::unsupported())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{
        ProtocolBootstrapKind,
        ProtocolDebugView,
        ProtocolDescriptor,
        ProtocolId,
        ProtocolImplementationKind,
        ProtocolTransportRequirements,
        ProtocolVersion,
    };
    struct LocalWordProtocol;

    impl Protocol for LocalWordProtocol {
        type Message = u32;

        const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
            id: ProtocolId(0x1A51_6A71_0000_0000_0000_0000_0000_0001),
            version: ProtocolVersion::new(1, 0, 0),
            caps: crate::protocol::ProtocolCaps::DEBUG_VIEW,
            bootstrap: ProtocolBootstrapKind::Immediate,
            debug_view: ProtocolDebugView::Structured,
            transport: ProtocolTransportRequirements::message_local(),
            implementation: ProtocolImplementationKind::Native,
        };
    }

    #[test]
    fn local_insight_channel_reports_feature_disabled_when_unavailable() {
        let support = LocalInsightChannel::<LocalWordProtocol, 4>::configured_support(
            InsightChannelClass::Timeline,
            InsightCaptureMode::Lossy,
        );

        #[cfg(feature = "debug-insights")]
        assert_eq!(support.availability, InsightAvailabilityKind::Available);
        #[cfg(not(feature = "debug-insights"))]
        assert_eq!(
            support.availability,
            InsightAvailabilityKind::DisabledByFeature
        );
    }

    #[cfg(not(feature = "debug-insights"))]
    #[test]
    fn local_insight_channel_construction_is_rejected_when_disabled() {
        let err = LocalInsightChannel::<LocalWordProtocol, 4>::new(
            InsightChannelClass::State,
            InsightCaptureMode::Exact,
        )
        .err()
        .expect("debug-insights should reject construction when disabled");

        assert_eq!(err.kind(), InsightErrorKind::NotEnabled);
    }

    #[cfg(not(feature = "debug-insights"))]
    #[test]
    fn local_insight_channel_stays_inactive_when_disabled() {
        assert_eq!(
            LocalInsightChannel::<LocalWordProtocol, 4>::configured_support(
                InsightChannelClass::Timeline,
                InsightCaptureMode::Lossy,
            )
            .availability,
            InsightAvailabilityKind::DisabledByFeature
        );
    }

    #[cfg(feature = "debug-insights")]
    #[test]
    fn local_insight_channel_round_trips_when_enabled() {
        let channel = LocalInsightChannel::<LocalWordProtocol, 4>::new(
            InsightChannelClass::Timeline,
            InsightCaptureMode::Lossy,
        )
        .expect("debug-insights channel should build");

        let producer = channel
            .attach_producer(crate::transport::TransportAttachmentRequest::same_courier())
            .expect("producer should attach");
        let consumer = channel
            .attach_consumer(crate::transport::TransportAttachmentRequest::same_courier())
            .expect("consumer should attach");

        channel
            .try_send(producer, 0xfeed_beef)
            .expect("send should work");
        assert_eq!(
            channel.try_receive(consumer).expect("receive should work"),
            Some(0xfeed_beef)
        );
        assert_eq!(
            channel.insight_support().availability,
            InsightAvailabilityKind::Available
        );
        assert_eq!(channel.class(), InsightChannelClass::Timeline);
        assert_eq!(channel.capture(), InsightCaptureMode::Lossy);
        assert_eq!(channel.producer_count(), 1);
        assert_eq!(channel.consumer_count(), 1);
        assert_eq!(channel.observation_state(), InsightObservationState::Active);
        assert_eq!(channel.observation_epoch(), 1);
        assert_eq!(
            channel.take_observation_transition(),
            Some(InsightObservationTransition::Activated)
        );
        assert_eq!(channel.take_observation_transition(), None);
    }

    #[cfg(feature = "debug-insights")]
    #[test]
    fn local_insight_channel_skips_lazy_send_when_unobserved() {
        let channel = LocalInsightChannel::<LocalWordProtocol, 4>::new(
            InsightChannelClass::Timeline,
            InsightCaptureMode::Lossy,
        )
        .expect("debug-insights channel should build");

        let producer = channel
            .attach_producer(crate::transport::TransportAttachmentRequest::same_courier())
            .expect("producer should attach");
        let mut built = false;

        assert!(!channel.is_observed());
        assert_eq!(
            channel
                .try_send_if_observed(producer, || {
                    built = true;
                    0xfeed_beef
                })
                .expect("lazy send should not fail"),
            false
        );
        assert!(!built);
    }

    #[cfg(feature = "debug-insights")]
    #[test]
    fn local_insight_channel_builds_lazy_payload_when_observed() {
        let channel = LocalInsightChannel::<LocalWordProtocol, 4>::new(
            InsightChannelClass::Timeline,
            InsightCaptureMode::Lossy,
        )
        .expect("debug-insights channel should build");

        let producer = channel
            .attach_producer(crate::transport::TransportAttachmentRequest::same_courier())
            .expect("producer should attach");
        let consumer = channel
            .attach_consumer(crate::transport::TransportAttachmentRequest::same_courier())
            .expect("consumer should attach");
        let mut built = false;

        assert!(channel.is_observed());
        assert_eq!(
            channel
                .try_send_if_observed(producer, || {
                    built = true;
                    0xfeed_beef
                })
                .expect("lazy send should succeed"),
            true
        );
        assert!(built);
        assert_eq!(
            channel.try_receive(consumer).expect("receive should work"),
            Some(0xfeed_beef)
        );
    }

    #[cfg(feature = "debug-insights")]
    #[test]
    fn local_insight_channel_tracks_observation_lifecycle_edges() {
        let channel = LocalInsightChannel::<LocalWordProtocol, 4>::new(
            InsightChannelClass::Timeline,
            InsightCaptureMode::Lossy,
        )
        .expect("debug-insights channel should build");

        assert_eq!(
            channel.observation_state(),
            InsightObservationState::Inactive
        );
        assert_eq!(channel.observation_epoch(), 0);
        assert_eq!(channel.take_observation_transition(), None);

        let first = channel
            .attach_consumer(crate::transport::TransportAttachmentRequest::same_courier())
            .expect("first consumer should attach");
        assert_eq!(channel.observation_state(), InsightObservationState::Active);
        assert_eq!(channel.observation_epoch(), 1);
        assert_eq!(
            channel.take_observation_transition(),
            Some(InsightObservationTransition::Activated)
        );
        assert_eq!(channel.take_observation_transition(), None);

        let second = channel
            .attach_consumer(crate::transport::TransportAttachmentRequest::same_courier())
            .expect("second consumer should attach");
        assert_eq!(channel.observation_state(), InsightObservationState::Active);
        assert_eq!(channel.observation_epoch(), 1);
        assert_eq!(channel.take_observation_transition(), None);

        channel
            .detach_consumer(first)
            .expect("first consumer should detach");
        assert_eq!(channel.observation_state(), InsightObservationState::Active);
        assert_eq!(channel.observation_epoch(), 1);
        assert_eq!(channel.take_observation_transition(), None);

        channel
            .detach_consumer(second)
            .expect("second consumer should detach");
        assert_eq!(
            channel.observation_state(),
            InsightObservationState::Inactive
        );
        assert_eq!(channel.observation_epoch(), 2);
        assert_eq!(
            channel.take_observation_transition(),
            Some(InsightObservationTransition::Deactivated)
        );
        assert_eq!(channel.take_observation_transition(), None);
    }

    #[cfg(all(feature = "debug-insights", feature = "std", not(target_os = "none")))]
    #[test]
    fn local_insight_channel_cross_thread_send_observes_and_delivers() {
        use std::sync::Arc;
        use std::thread;

        let channel = Arc::new(
            LocalInsightChannel::<LocalWordProtocol, 4>::new(
                InsightChannelClass::Timeline,
                InsightCaptureMode::Lossy,
            )
            .expect("debug-insights channel should build"),
        );
        let producer = channel
            .attach_producer(crate::transport::TransportAttachmentRequest::cross_courier())
            .expect("producer should attach");
        let consumer = channel
            .attach_consumer(crate::transport::TransportAttachmentRequest::cross_courier())
            .expect("consumer should attach");
        let sender = Arc::clone(&channel);

        let thread = thread::spawn(move || {
            assert!(
                sender
                    .try_send_if_observed(producer, || 0xCAFE_BABE)
                    .expect("cross-thread lazy send should succeed"),
            );
        });

        thread.join().expect("sender thread should finish");
        assert_eq!(
            channel
                .try_receive(consumer)
                .expect("receive should succeed"),
            Some(0xCAFE_BABE)
        );
    }
}
