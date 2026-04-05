//! Timeline/flame-style span capture over one local insight side channel.
//!
//! This sits above the raw observer lifecycle in [`LocalInsightChannel`]. The channel tracks
//! attach/detach truth; this layer turns that into coherent span sessions so a disappearing
//! listener does not trick the runtime into starting new spans while old ones are still draining.

use core::cell::Cell;
use core::marker::PhantomData;

use crate::channel::{
    ChannelError,
    ChannelReceiveContract,
};
use crate::channel::insight::{
    InsightCaptureMode,
    InsightChannelClass,
    InsightError,
    InsightObservationTransition,
    LocalInsightChannel,
};
use crate::transport::protocol::{
    ProtocolContract,
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
    TransportAttachmentControlContract,
    TransportAttachmentRequest,
    TransportError,
};

/// One timeline/flamegraph span identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InsightTimelineSpanId(pub u64);

/// One live timeline span token returned by [`LocalInsightTimeline::begin_span`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InsightTimelineSpanToken {
    span: InsightTimelineSpanId,
    epoch: u64,
}

impl InsightTimelineSpanToken {
    /// Returns the span identifier visible on the wire.
    #[must_use]
    pub const fn id(self) -> InsightTimelineSpanId {
        self.span
    }

    /// Returns the capture epoch that created this span.
    #[must_use]
    pub const fn epoch(self) -> u64 {
        self.epoch
    }
}

/// One timeline/span record emitted over the timeline insight channel.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InsightTimelineRecord<Meta> {
    /// One new span opened while capture was active.
    SpanOpened {
        epoch: u64,
        span: InsightTimelineSpanId,
        parent: Option<InsightTimelineSpanId>,
        meta: Meta,
    },
    /// One previously opened span closed while capture remained active.
    SpanClosed {
        epoch: u64,
        span: InsightTimelineSpanId,
    },
}

/// Built-in protocol for timeline/flamegraph span records.
pub struct InsightTimelineProtocol<Meta>(PhantomData<Meta>);

impl<Meta> ProtocolContract for InsightTimelineProtocol<Meta> {
    type Message = InsightTimelineRecord<Meta>;

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
        id: ProtocolId(0x4655_5349_4f4e_494e_5349_4748_545f_0002),
        version: ProtocolVersion::new(1, 0, 0),
        caps: ProtocolCaps::DEBUG_VIEW,
        bootstrap: ProtocolBootstrapKind::Immediate,
        debug_view: ProtocolDebugView::Structured,
        transport: ProtocolTransportRequirements::message_local(),
        implementation: ProtocolImplementationKind::Native,
    };
}

/// Lifecycle state for one local timeline capture session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InsightTimelineCaptureState {
    /// No listener is active and no prior spans are draining.
    Inactive,
    /// One or more listeners are attached and new spans may be emitted.
    Active { epoch: u64 },
    /// No listeners remain, but older spans are still being closed out internally.
    Draining { epoch: u64 },
}

impl InsightTimelineCaptureState {
    #[must_use]
    const fn epoch(self) -> Option<u64> {
        match self {
            Self::Inactive => None,
            Self::Active { epoch } | Self::Draining { epoch } => Some(epoch),
        }
    }
}

/// Local timeline/flamegraph session service over one insight timeline channel.
///
/// The service owns the producer attachment. Consumers may attach and detach freely; the service
/// promotes observation transitions into coherent capture sessions so:
/// - the first observer activates capture
/// - the last observer stops new spans
/// - spans already opened in that session may still close internally while the service drains
pub struct LocalInsightTimeline<Meta, const CAPACITY: usize, const MAX_CONSUMERS: usize = 8> {
    channel: LocalInsightChannel<InsightTimelineProtocol<Meta>, CAPACITY, MAX_CONSUMERS>,
    producer: usize,
    capture_state: Cell<InsightTimelineCaptureState>,
    next_span_id: Cell<u64>,
    active_span_count: Cell<usize>,
}

impl<Meta, const CAPACITY: usize, const MAX_CONSUMERS: usize>
    LocalInsightTimeline<Meta, CAPACITY, MAX_CONSUMERS>
{
    /// Creates one local timeline/flamegraph service.
    ///
    /// # Errors
    ///
    /// Returns `InsightError::not_enabled()` when `debug-insights` is disabled, or an honest
    /// insight-channel failure if the internal producer attachment cannot be established.
    pub fn new(capture: InsightCaptureMode) -> Result<Self, InsightError> {
        let channel =
            LocalInsightChannel::<InsightTimelineProtocol<Meta>, CAPACITY, MAX_CONSUMERS>::new(
                InsightChannelClass::Timeline,
                capture,
            )?;
        let producer = channel
            .attach_producer(TransportAttachmentRequest::same_courier())
            .map_err(|error| InsightError::from(ChannelError::from(error)))?;
        Ok(Self {
            channel,
            producer,
            capture_state: Cell::new(InsightTimelineCaptureState::Inactive),
            next_span_id: Cell::new(1),
            active_span_count: Cell::new(0),
        })
    }

    fn sync_observation(&self) {
        while let Some(transition) = self.channel.take_observation_transition() {
            match transition {
                InsightObservationTransition::Activated => {
                    // A new observer session starts here. Any prior draining spans belonged to the
                    // old disconnected session and should not leak into the new one.
                    let _ = self.channel.clear_pending_messages();
                    self.active_span_count.set(0);
                    self.capture_state.set(InsightTimelineCaptureState::Active {
                        epoch: self.channel.observation_epoch(),
                    });
                }
                InsightObservationTransition::Deactivated => {
                    let epoch = self
                        .capture_state
                        .get()
                        .epoch()
                        .unwrap_or(self.channel.observation_epoch());
                    let _ = self.channel.clear_pending_messages();
                    if self.active_span_count.get() == 0 {
                        self.capture_state
                            .set(InsightTimelineCaptureState::Inactive);
                    } else {
                        self.capture_state
                            .set(InsightTimelineCaptureState::Draining { epoch });
                    }
                }
            }
        }
    }

    fn current_epoch(&self) -> Option<u64> {
        self.capture_state.get().epoch()
    }

    fn finish_span_without_emit(&self, token: InsightTimelineSpanToken) {
        let Some(epoch) = self.current_epoch() else {
            return;
        };
        if token.epoch != epoch {
            return;
        }
        let active = self.active_span_count.get();
        debug_assert!(
            active != 0,
            "timeline span close underflow: no active spans remained for token {:?}",
            token
        );
        if active == 0 {
            return;
        }
        self.active_span_count.set(active - 1);
        if self.active_span_count.get() == 0
            && matches!(
                self.capture_state.get(),
                InsightTimelineCaptureState::Draining { .. }
            )
        {
            self.capture_state
                .set(InsightTimelineCaptureState::Inactive);
        }
    }

    /// Returns the underlying timeline insight channel.
    #[must_use]
    pub const fn channel(
        &self,
    ) -> &LocalInsightChannel<InsightTimelineProtocol<Meta>, CAPACITY, MAX_CONSUMERS> {
        &self.channel
    }

    /// Returns the current timeline capture state.
    #[must_use]
    pub fn capture_state(&self) -> InsightTimelineCaptureState {
        self.sync_observation();
        self.capture_state.get()
    }

    /// Opens one new span when capture is active.
    ///
    /// Returns `Ok(None)` when no listeners are attached or when the timeline is draining a prior
    /// disconnected session.
    pub fn begin_span(
        &self,
        parent: Option<InsightTimelineSpanToken>,
        meta: Meta,
    ) -> Result<Option<InsightTimelineSpanToken>, ChannelError> {
        self.sync_observation();
        let InsightTimelineCaptureState::Active { epoch } = self.capture_state.get() else {
            return Ok(None);
        };

        let span = InsightTimelineSpanId(self.next_span_id.get());
        self.next_span_id
            .set(self.next_span_id.get().wrapping_add(1));
        let parent = parent
            .filter(|token| token.epoch == epoch)
            .map(InsightTimelineSpanToken::id);

        match self.channel.try_send_if_observed(self.producer, || {
            InsightTimelineRecord::SpanOpened {
                epoch,
                span,
                parent,
                meta,
            }
        })? {
            true => {
                self.active_span_count
                    .set(self.active_span_count.get().saturating_add(1));
                Ok(Some(InsightTimelineSpanToken { span, epoch }))
            }
            false => {
                self.sync_observation();
                Ok(None)
            }
        }
    }

    /// Closes one previously opened span.
    ///
    /// Stale spans from a prior capture epoch are ignored. Spans closed while the timeline is
    /// draining are retired internally without emitting new records.
    pub fn end_span(&self, token: InsightTimelineSpanToken) -> Result<(), ChannelError> {
        self.sync_observation();
        let Some(epoch) = self.current_epoch() else {
            return Ok(());
        };
        if token.epoch != epoch {
            return Ok(());
        }

        match self.capture_state.get() {
            InsightTimelineCaptureState::Inactive => Ok(()),
            InsightTimelineCaptureState::Draining { .. } => {
                self.finish_span_without_emit(token);
                Ok(())
            }
            InsightTimelineCaptureState::Active { .. } => {
                let _ = self.channel.try_send_if_observed(self.producer, || {
                    InsightTimelineRecord::SpanClosed {
                        epoch,
                        span: token.id(),
                    }
                })?;
                self.finish_span_without_emit(token);
                Ok(())
            }
        }
    }

    /// Attaches one consumer to the underlying timeline channel.
    pub fn attach_consumer(
        &self,
        request: TransportAttachmentRequest,
    ) -> Result<usize, TransportError> {
        self.channel.attach_consumer(request)
    }

    /// Detaches one consumer from the underlying timeline channel.
    pub fn detach_consumer(&self, attachment: usize) -> Result<(), TransportError> {
        self.channel.detach_consumer(attachment)
    }

    /// Receives one timeline record from the underlying channel.
    pub fn try_receive(
        &self,
        consumer: usize,
    ) -> Result<Option<InsightTimelineRecord<Meta>>, ChannelError> {
        self.channel.try_receive(consumer)
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "debug-insights")]
    use super::*;

    #[cfg(feature = "debug-insights")]
    #[test]
    fn timeline_stays_dormant_until_observed() {
        let timeline =
            LocalInsightTimeline::<&'static str, 8>::new(InsightCaptureMode::Lossy).unwrap();

        assert_eq!(
            timeline.capture_state(),
            InsightTimelineCaptureState::Inactive
        );
        assert_eq!(timeline.begin_span(None, "cold").unwrap(), None);
        assert_eq!(
            timeline.capture_state(),
            InsightTimelineCaptureState::Inactive
        );
    }

    #[cfg(feature = "debug-insights")]
    #[test]
    fn timeline_emits_open_and_close_when_observed() {
        let timeline =
            LocalInsightTimeline::<&'static str, 8>::new(InsightCaptureMode::Lossy).unwrap();
        let consumer = timeline
            .attach_consumer(TransportAttachmentRequest::same_courier())
            .expect("consumer should attach");

        let root = timeline
            .begin_span(None, "root")
            .expect("begin should work")
            .expect("observed timeline should open spans");
        let child = timeline
            .begin_span(Some(root), "child")
            .expect("begin should work")
            .expect("observed timeline should open child spans");

        assert_eq!(
            timeline.try_receive(consumer).expect("receive should work"),
            Some(InsightTimelineRecord::SpanOpened {
                epoch: root.epoch(),
                span: root.id(),
                parent: None,
                meta: "root",
            })
        );
        assert_eq!(
            timeline.try_receive(consumer).expect("receive should work"),
            Some(InsightTimelineRecord::SpanOpened {
                epoch: child.epoch(),
                span: child.id(),
                parent: Some(root.id()),
                meta: "child",
            })
        );

        timeline.end_span(child).expect("end should work");
        timeline.end_span(root).expect("end should work");

        assert_eq!(
            timeline.try_receive(consumer).expect("receive should work"),
            Some(InsightTimelineRecord::SpanClosed {
                epoch: child.epoch(),
                span: child.id(),
            })
        );
        assert_eq!(
            timeline.try_receive(consumer).expect("receive should work"),
            Some(InsightTimelineRecord::SpanClosed {
                epoch: root.epoch(),
                span: root.id(),
            })
        );
        assert_eq!(
            timeline.try_receive(consumer).expect("receive should work"),
            None
        );
        assert_eq!(
            timeline.capture_state(),
            InsightTimelineCaptureState::Active { epoch: 1 }
        );
    }

    #[cfg(feature = "debug-insights")]
    #[test]
    fn timeline_enters_draining_when_last_listener_detaches() {
        let timeline =
            LocalInsightTimeline::<&'static str, 8>::new(InsightCaptureMode::Lossy).unwrap();
        let consumer = timeline
            .attach_consumer(TransportAttachmentRequest::same_courier())
            .expect("consumer should attach");
        let span = timeline
            .begin_span(None, "root")
            .expect("begin should work")
            .expect("span should open");

        timeline
            .detach_consumer(consumer)
            .expect("consumer should detach");
        assert_eq!(
            timeline.capture_state(),
            InsightTimelineCaptureState::Draining { epoch: 1 }
        );
        assert_eq!(timeline.begin_span(None, "blocked").unwrap(), None);

        timeline.end_span(span).expect("end should work");
        assert_eq!(
            timeline.capture_state(),
            InsightTimelineCaptureState::Inactive
        );
    }

    #[cfg(feature = "debug-insights")]
    #[test]
    fn timeline_restarts_cleanly_after_draining_session() {
        let timeline =
            LocalInsightTimeline::<&'static str, 8>::new(InsightCaptureMode::Lossy).unwrap();
        let first_consumer = timeline
            .attach_consumer(TransportAttachmentRequest::same_courier())
            .expect("consumer should attach");
        let stale = timeline
            .begin_span(None, "stale")
            .expect("begin should work")
            .expect("span should open");
        timeline
            .detach_consumer(first_consumer)
            .expect("consumer should detach");
        assert_eq!(
            timeline.capture_state(),
            InsightTimelineCaptureState::Draining { epoch: 1 }
        );

        let second_consumer = timeline
            .attach_consumer(TransportAttachmentRequest::same_courier())
            .expect("second consumer should attach");
        assert_eq!(
            timeline.capture_state(),
            InsightTimelineCaptureState::Active { epoch: 3 }
        );

        timeline
            .end_span(stale)
            .expect("stale span closure should be ignored");

        let fresh = timeline
            .begin_span(None, "fresh")
            .expect("begin should work")
            .expect("fresh span should open");
        assert_eq!(
            timeline
                .try_receive(second_consumer)
                .expect("receive should work"),
            Some(InsightTimelineRecord::SpanOpened {
                epoch: fresh.epoch(),
                span: fresh.id(),
                parent: None,
                meta: "fresh",
            })
        );
    }
}
