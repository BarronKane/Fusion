//! Channel-operable audit surface for combined current-thread fiber + async runtimes.
//!
//! Direct runtime APIs remain first-class. This module exists so configured runtime sizing truth
//! can cross one honest protocol/transport/channel boundary when callers actually need that
//! composition shape.

use core::pin::Pin;

use fusion_sys::channel::{
    ChannelError,
    ChannelErrorKind,
    ChannelReceive,
    ChannelSend,
    LocalChannel,
};
use fusion_sys::fiber::{
    Fiber,
    FiberError,
    FiberReturn,
    FiberRunnable,
    FiberStack,
    ManagedFiber,
    yield_now,
};
use fusion_sys::insight::{
    InsightCaptureMode,
    InsightChannelClass,
    InsightError,
    LocalInsightChannel,
};
use fusion_sys::protocol::{
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
use fusion_sys::transport::{
    TransportAttachmentControl,
    TransportAttachmentRequest,
    TransportDirection,
    TransportError,
    TransportErrorKind,
    TransportFraming,
};

use super::{
    CurrentFiberAsyncRuntime,
    CurrentFiberAsyncRuntimeError,
    CurrentFiberAsyncRuntimeMemoryFootprint,
};

/// State insight record for one combined current-thread fiber + async runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CurrentFiberAsyncRuntimeStateRecord {
    Configured(CurrentFiberAsyncRuntimeInfo),
}

/// Snapshot insight record for one combined current-thread fiber + async runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CurrentFiberAsyncRuntimeSnapshotRecord {
    ConfiguredMemoryFootprint(CurrentFiberAsyncRuntimeMemoryFootprint),
}

/// Public runtime metadata surfaced over the runtime audit metadata channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CurrentFiberAsyncRuntimeInfo {
    /// Number of fiber carriers provisioned for the runtime.
    pub fiber_carrier_count: usize,
    /// Total schedulable fiber task capacity.
    pub fiber_task_capacity: usize,
    /// Fixed async task-registry capacity.
    pub executor_task_capacity: usize,
}

/// Metadata snapshot/event for one combined current-thread fiber + async runtime surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CurrentFiberAsyncRuntimeMetadataMessage {
    Advertised(CurrentFiberAsyncRuntimeInfo),
}

/// Control request sent to one combined current-thread fiber + async runtime audit service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CurrentFiberAsyncRuntimeControlRequest {
    ReadConfiguredMemoryFootprint,
    RepublishMetadata,
}

/// Control/status message emitted by one combined current-thread fiber + async runtime audit
/// service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CurrentFiberAsyncRuntimeControlStatusMessage {
    ConfiguredMemoryFootprint(CurrentFiberAsyncRuntimeMemoryFootprint),
    MetadataRepublishScheduled,
    Rejected {
        reason: CurrentFiberAsyncRuntimeError,
    },
}

/// Metadata/read protocol for one combined current-thread fiber + async runtime audit surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CurrentFiberAsyncRuntimeMetadataProtocol;

impl Protocol for CurrentFiberAsyncRuntimeMetadataProtocol {
    type Message = CurrentFiberAsyncRuntimeMetadataMessage;

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
        id: ProtocolId(0x4655_5349_4f4e_5254_4658_5f4d_445f_0001),
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

/// Control/write protocol for one combined current-thread fiber + async runtime audit surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CurrentFiberAsyncRuntimeControlWriteProtocol;

impl Protocol for CurrentFiberAsyncRuntimeControlWriteProtocol {
    type Message = CurrentFiberAsyncRuntimeControlRequest;

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
        id: ProtocolId(0x4655_5349_4f4e_5254_4658_5f43_545f_0001),
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

/// Status/read protocol for one combined current-thread fiber + async runtime audit surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CurrentFiberAsyncRuntimeControlStatusProtocol;

impl Protocol for CurrentFiberAsyncRuntimeControlStatusProtocol {
    type Message = CurrentFiberAsyncRuntimeControlStatusMessage;

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
        id: ProtocolId(0x4655_5349_4f4e_5254_4658_5f53_545f_0001),
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

/// Insight protocol for current runtime state records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CurrentFiberAsyncRuntimeStateInsightProtocol;

impl Protocol for CurrentFiberAsyncRuntimeStateInsightProtocol {
    type Message = CurrentFiberAsyncRuntimeStateRecord;

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
        id: ProtocolId(0x4655_5349_4f4e_5254_4658_5f49_5354_0001),
        version: ProtocolVersion::new(1, 0, 0),
        caps: ProtocolCaps::DEBUG_VIEW,
        bootstrap: ProtocolBootstrapKind::Immediate,
        debug_view: ProtocolDebugView::Structured,
        transport: ProtocolTransportRequirements::message_local(),
        implementation: ProtocolImplementationKind::Native,
    };
}

/// Insight protocol for current runtime snapshot records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CurrentFiberAsyncRuntimeSnapshotInsightProtocol;

impl Protocol for CurrentFiberAsyncRuntimeSnapshotInsightProtocol {
    type Message = CurrentFiberAsyncRuntimeSnapshotRecord;

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
        id: ProtocolId(0x4655_5349_4f4e_5254_4658_5f49_534e_0001),
        version: ProtocolVersion::new(1, 0, 0),
        caps: ProtocolCaps::DEBUG_VIEW,
        bootstrap: ProtocolBootstrapKind::Immediate,
        debug_view: ProtocolDebugView::Structured,
        transport: ProtocolTransportRequirements::message_local(),
        implementation: ProtocolImplementationKind::Native,
    };
}

/// Optional insight side channels for one combined current-thread fiber + async runtime.
///
/// The front door is always present. When `debug-insights` is disabled, construction is rejected
/// honestly; when enabled, payload construction remains observer-gated so unobserved insight stays
/// essentially free.
pub struct CurrentFiberAsyncRuntimeInsight<
    const STATE_CAPACITY: usize = 4,
    const SNAPSHOT_CAPACITY: usize = 4,
    const MAX_CONSUMERS: usize = 8,
> {
    state: LocalInsightChannel<
        CurrentFiberAsyncRuntimeStateInsightProtocol,
        STATE_CAPACITY,
        MAX_CONSUMERS,
    >,
    snapshot: LocalInsightChannel<
        CurrentFiberAsyncRuntimeSnapshotInsightProtocol,
        SNAPSHOT_CAPACITY,
        MAX_CONSUMERS,
    >,
    state_producer: usize,
    snapshot_producer: usize,
}

impl<const STATE_CAPACITY: usize, const SNAPSHOT_CAPACITY: usize, const MAX_CONSUMERS: usize>
    CurrentFiberAsyncRuntimeInsight<STATE_CAPACITY, SNAPSHOT_CAPACITY, MAX_CONSUMERS>
{
    /// Creates one runtime insight surface.
    ///
    /// # Errors
    ///
    /// Returns `InsightError::not_enabled()` when `debug-insights` is disabled, or an honest
    /// insight-channel failure when producer attachment cannot be established.
    pub fn new(capture: InsightCaptureMode) -> Result<Self, InsightError> {
        let state = LocalInsightChannel::<
            CurrentFiberAsyncRuntimeStateInsightProtocol,
            STATE_CAPACITY,
            MAX_CONSUMERS,
        >::new(InsightChannelClass::State, capture)?;
        let snapshot = LocalInsightChannel::<
            CurrentFiberAsyncRuntimeSnapshotInsightProtocol,
            SNAPSHOT_CAPACITY,
            MAX_CONSUMERS,
        >::new(InsightChannelClass::Snapshot, capture)?;
        let request = TransportAttachmentRequest::same_courier();
        let state_producer = state
            .attach_producer(request)
            .map_err(|error| InsightError::from(ChannelError::from(error)))?;
        let snapshot_producer = snapshot
            .attach_producer(request)
            .map_err(|error| InsightError::from(ChannelError::from(error)))?;

        Ok(Self {
            state,
            snapshot,
            state_producer,
            snapshot_producer,
        })
    }

    /// Returns the runtime state insight channel.
    #[must_use]
    pub const fn state_channel(
        &self,
    ) -> &LocalInsightChannel<
        CurrentFiberAsyncRuntimeStateInsightProtocol,
        STATE_CAPACITY,
        MAX_CONSUMERS,
    > {
        &self.state
    }

    /// Returns the runtime snapshot insight channel.
    #[must_use]
    pub const fn snapshot_channel(
        &self,
    ) -> &LocalInsightChannel<
        CurrentFiberAsyncRuntimeSnapshotInsightProtocol,
        SNAPSHOT_CAPACITY,
        MAX_CONSUMERS,
    > {
        &self.snapshot
    }

    /// Emits one configured runtime-state record only when the state channel is observed.
    pub fn emit_state_if_observed(
        &self,
        runtime: &CurrentFiberAsyncRuntime,
    ) -> Result<bool, ChannelError> {
        self.state.try_send_if_observed(self.state_producer, || {
            CurrentFiberAsyncRuntimeStateRecord::Configured(runtime_info(runtime))
        })
    }

    /// Emits one configured memory-footprint snapshot only when the snapshot channel is observed.
    pub fn emit_configured_memory_footprint_if_observed(
        &self,
        runtime: &CurrentFiberAsyncRuntime,
    ) -> Result<bool, CurrentFiberAsyncRuntimeChannelServiceError> {
        if !self.snapshot.is_observed() {
            return Ok(false);
        }
        let footprint = runtime.configured_memory_footprint()?;
        self.snapshot
            .try_send_if_observed(self.snapshot_producer, || {
                CurrentFiberAsyncRuntimeSnapshotRecord::ConfiguredMemoryFootprint(footprint)
            })
            .map_err(CurrentFiberAsyncRuntimeChannelServiceError::from)
    }
}

/// Error surfaced while constructing or pumping one combined current-thread fiber + async runtime
/// audit service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CurrentFiberAsyncRuntimeChannelServiceError {
    kind: CurrentFiberAsyncRuntimeChannelServiceErrorKind,
}

impl CurrentFiberAsyncRuntimeChannelServiceError {
    /// Returns the concrete service error kind.
    #[must_use]
    pub const fn kind(self) -> CurrentFiberAsyncRuntimeChannelServiceErrorKind {
        self.kind
    }
}

/// Classification of combined current-thread fiber + async runtime audit service failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CurrentFiberAsyncRuntimeChannelServiceErrorKind {
    Runtime(CurrentFiberAsyncRuntimeError),
    Channel(ChannelErrorKind),
    Transport(TransportErrorKind),
}

impl From<CurrentFiberAsyncRuntimeError> for CurrentFiberAsyncRuntimeChannelServiceError {
    fn from(value: CurrentFiberAsyncRuntimeError) -> Self {
        Self {
            kind: CurrentFiberAsyncRuntimeChannelServiceErrorKind::Runtime(value),
        }
    }
}

impl From<ChannelError> for CurrentFiberAsyncRuntimeChannelServiceError {
    fn from(value: ChannelError) -> Self {
        Self {
            kind: CurrentFiberAsyncRuntimeChannelServiceErrorKind::Channel(value.kind()),
        }
    }
}

impl From<TransportError> for CurrentFiberAsyncRuntimeChannelServiceError {
    fn from(value: TransportError) -> Self {
        Self {
            kind: CurrentFiberAsyncRuntimeChannelServiceErrorKind::Transport(value.kind()),
        }
    }
}

/// Same-context runtime audit service over local one-way channels.
pub struct CurrentFiberAsyncRuntimeChannelService<
    'a,
    const METADATA_CAPACITY: usize = 4,
    const CONTROL_CAPACITY: usize = 4,
    const STATUS_CAPACITY: usize = 4,
> {
    runtime: &'a CurrentFiberAsyncRuntime,
    metadata_dirty: bool,
    pending_status: Option<CurrentFiberAsyncRuntimeControlStatusMessage>,
    metadata_channel: LocalChannel<CurrentFiberAsyncRuntimeMetadataProtocol, METADATA_CAPACITY>,
    control_channel: LocalChannel<CurrentFiberAsyncRuntimeControlWriteProtocol, CONTROL_CAPACITY>,
    status_channel: LocalChannel<CurrentFiberAsyncRuntimeControlStatusProtocol, STATUS_CAPACITY>,
    metadata_producer: usize,
    control_consumer: usize,
    status_producer: usize,
}

impl<
    'a,
    const METADATA_CAPACITY: usize,
    const CONTROL_CAPACITY: usize,
    const STATUS_CAPACITY: usize,
> CurrentFiberAsyncRuntimeChannelService<'a, METADATA_CAPACITY, CONTROL_CAPACITY, STATUS_CAPACITY>
{
    /// Creates one local runtime audit service for one combined current-thread fiber + async
    /// runtime.
    ///
    /// # Errors
    ///
    /// Returns an honest channel-transport failure when the local channels cannot be instantiated
    /// or attached.
    pub fn new(
        runtime: &'a CurrentFiberAsyncRuntime,
    ) -> Result<Self, CurrentFiberAsyncRuntimeChannelServiceError> {
        let metadata_channel =
            LocalChannel::<CurrentFiberAsyncRuntimeMetadataProtocol, METADATA_CAPACITY>::new()?;
        let control_channel =
            LocalChannel::<CurrentFiberAsyncRuntimeControlWriteProtocol, CONTROL_CAPACITY>::new()?;
        let status_channel =
            LocalChannel::<CurrentFiberAsyncRuntimeControlStatusProtocol, STATUS_CAPACITY>::new()?;
        let request = TransportAttachmentRequest::same_courier();

        let metadata_producer = metadata_channel.attach_producer(request)?;
        let control_consumer = control_channel.attach_consumer(request)?;
        let status_producer = status_channel.attach_producer(request)?;

        Ok(Self {
            runtime,
            metadata_dirty: true,
            pending_status: None,
            metadata_channel,
            control_channel,
            status_channel,
            metadata_producer,
            control_consumer,
            status_producer,
        })
    }

    /// Returns the runtime metadata channel.
    #[must_use]
    pub const fn metadata_channel(
        &self,
    ) -> &LocalChannel<CurrentFiberAsyncRuntimeMetadataProtocol, METADATA_CAPACITY> {
        &self.metadata_channel
    }

    /// Returns the runtime control channel.
    #[must_use]
    pub const fn control_channel(
        &self,
    ) -> &LocalChannel<CurrentFiberAsyncRuntimeControlWriteProtocol, CONTROL_CAPACITY> {
        &self.control_channel
    }

    /// Returns the runtime status channel.
    #[must_use]
    pub const fn status_channel(
        &self,
    ) -> &LocalChannel<CurrentFiberAsyncRuntimeControlStatusProtocol, STATUS_CAPACITY> {
        &self.status_channel
    }

    /// Spawns this audit service on one managed fiber with an automatic metadata channel.
    ///
    /// # Errors
    ///
    /// Returns any honest low-level fiber construction failure.
    pub fn spawn_managed<'state, const META_FIBER_CAPACITY: usize, const MAX_CONSUMERS: usize>(
        state: Pin<&'state mut Self>,
        stack: FiberStack,
    ) -> Result<ManagedFiber<'state, Self, META_FIBER_CAPACITY, MAX_CONSUMERS>, FiberError> {
        Fiber::spawn_managed(stack, state)
    }

    /// Pumps pending metadata and control requests once.
    ///
    /// # Errors
    ///
    /// Returns an honest channel or runtime failure when the service cannot make forward progress
    /// honestly.
    pub fn pump(&mut self) -> Result<(), CurrentFiberAsyncRuntimeChannelServiceError> {
        self.flush_metadata()?;
        self.flush_pending_status()?;

        if self.pending_status.is_some() {
            return Ok(());
        }

        while let Some(request) = self.control_channel.try_receive(self.control_consumer)? {
            self.handle_request(request)?;
            self.flush_pending_status()?;
            self.flush_metadata()?;
            if self.pending_status.is_some() {
                break;
            }
        }

        Ok(())
    }

    fn flush_metadata(&mut self) -> Result<(), CurrentFiberAsyncRuntimeChannelServiceError> {
        if !self.metadata_dirty {
            return Ok(());
        }
        match self.metadata_channel.try_send(
            self.metadata_producer,
            CurrentFiberAsyncRuntimeMetadataMessage::Advertised(runtime_info(self.runtime)),
        ) {
            Ok(()) => {
                self.metadata_dirty = false;
                Ok(())
            }
            Err(error)
                if matches!(
                    error.kind(),
                    ChannelErrorKind::Busy | ChannelErrorKind::ResourceExhausted
                ) =>
            {
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    }

    fn flush_pending_status(&mut self) -> Result<(), CurrentFiberAsyncRuntimeChannelServiceError> {
        let Some(message) = self.pending_status else {
            return Ok(());
        };
        match self.status_channel.try_send(self.status_producer, message) {
            Ok(()) => {
                self.pending_status = None;
                Ok(())
            }
            Err(error)
                if matches!(
                    error.kind(),
                    ChannelErrorKind::Busy | ChannelErrorKind::ResourceExhausted
                ) =>
            {
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    }

    fn handle_request(
        &mut self,
        request: CurrentFiberAsyncRuntimeControlRequest,
    ) -> Result<(), CurrentFiberAsyncRuntimeChannelServiceError> {
        self.pending_status = Some(match request {
            CurrentFiberAsyncRuntimeControlRequest::ReadConfiguredMemoryFootprint => {
                match self.runtime.configured_memory_footprint() {
                    Ok(footprint) => {
                        CurrentFiberAsyncRuntimeControlStatusMessage::ConfiguredMemoryFootprint(
                            footprint,
                        )
                    }
                    Err(error) => {
                        CurrentFiberAsyncRuntimeControlStatusMessage::Rejected { reason: error }
                    }
                }
            }
            CurrentFiberAsyncRuntimeControlRequest::RepublishMetadata => {
                self.metadata_dirty = true;
                CurrentFiberAsyncRuntimeControlStatusMessage::MetadataRepublishScheduled
            }
        });
        Ok(())
    }
}

impl<
    'a,
    const METADATA_CAPACITY: usize,
    const CONTROL_CAPACITY: usize,
    const STATUS_CAPACITY: usize,
> FiberRunnable
    for CurrentFiberAsyncRuntimeChannelService<
        'a,
        METADATA_CAPACITY,
        CONTROL_CAPACITY,
        STATUS_CAPACITY,
    >
{
    fn run(mut self: Pin<&mut Self>) -> FiberReturn {
        let service = self.as_mut().get_mut();
        loop {
            if service.pump().is_err() {
                return FiberReturn::new(1);
            }
            if yield_now().is_err() {
                return FiberReturn::new(2);
            }
        }
    }
}

fn runtime_info(runtime: &CurrentFiberAsyncRuntime) -> CurrentFiberAsyncRuntimeInfo {
    let fibers = runtime.fibers().memory_footprint();
    CurrentFiberAsyncRuntimeInfo {
        fiber_carrier_count: fibers.carrier_count,
        fiber_task_capacity: fibers.task_capacity,
        executor_task_capacity: runtime.executor().config().capacity,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thread::CurrentFiberAsyncBootstrap;
    use core::num::NonZeroUsize;
    use core::pin::pin;
    use fusion_sys::channel::{ChannelReceive, ChannelSend};
    use fusion_sys::fiber::{FiberMetadataMessage, FiberStack, FiberYield};
    use fusion_sys::transport::{TransportAttachmentControl, TransportAttachmentRequest};
    use std::vec;

    #[test]
    fn runtime_channel_service_advertises_and_reports_configured_footprint() {
        let runtime = CurrentFiberAsyncBootstrap::uniform(
            1,
            NonZeroUsize::new(8 * 1024).expect("non-zero stack"),
            2,
        )
        .with_guard_pages(0)
        .build_current()
        .expect("combined runtime should build");

        {
            let expected = runtime
                .configured_memory_footprint()
                .expect("configured footprint should build");
            let mut service: CurrentFiberAsyncRuntimeChannelService<'_, 4, 4, 4> =
                CurrentFiberAsyncRuntimeChannelService::new(&runtime)
                    .expect("runtime channel service should build");
            let metadata_consumer = service
                .metadata_channel()
                .attach_consumer(TransportAttachmentRequest::same_courier())
                .expect("metadata consumer should attach");
            let control_producer = service
                .control_channel()
                .attach_producer(TransportAttachmentRequest::same_courier())
                .expect("control producer should attach");
            let status_consumer = service
                .status_channel()
                .attach_consumer(TransportAttachmentRequest::same_courier())
                .expect("status consumer should attach");

            service.pump().expect("service should publish metadata");
            let metadata = service
                .metadata_channel()
                .try_receive(metadata_consumer)
                .expect("metadata receive should succeed")
                .expect("metadata message should exist");
            match metadata {
                CurrentFiberAsyncRuntimeMetadataMessage::Advertised(info) => {
                    assert_eq!(info.executor_task_capacity, 2);
                    assert_eq!(info.fiber_carrier_count, 1);
                    assert!(info.fiber_task_capacity >= 1);
                }
            }

            service
                .control_channel()
                .try_send(
                    control_producer,
                    CurrentFiberAsyncRuntimeControlRequest::ReadConfiguredMemoryFootprint,
                )
                .expect("configured footprint request should send");
            service
                .pump()
                .expect("service should handle configured footprint request");
            let status = service
                .status_channel()
                .try_receive(status_consumer)
                .expect("status receive should succeed")
                .expect("status message should exist");
            match status {
                CurrentFiberAsyncRuntimeControlStatusMessage::ConfiguredMemoryFootprint(
                    footprint,
                ) => {
                    assert_eq!(footprint, expected);
                }
                other => panic!("unexpected runtime status: {other:?}"),
            }

            service
                .control_channel()
                .try_send(
                    control_producer,
                    CurrentFiberAsyncRuntimeControlRequest::RepublishMetadata,
                )
                .expect("republish request should send");
            service.pump().expect("service should republish metadata");

            let republished = service
                .status_channel()
                .try_receive(status_consumer)
                .expect("republish status receive should succeed")
                .expect("republish status should exist");
            assert_eq!(
                republished,
                CurrentFiberAsyncRuntimeControlStatusMessage::MetadataRepublishScheduled
            );

            let metadata = service
                .metadata_channel()
                .try_receive(metadata_consumer)
                .expect("republished metadata receive should succeed")
                .expect("republished metadata should exist");
            match metadata {
                CurrentFiberAsyncRuntimeMetadataMessage::Advertised(info) => {
                    assert_eq!(info.executor_task_capacity, 2);
                    assert_eq!(info.fiber_carrier_count, 1);
                    assert!(info.fiber_task_capacity >= 1);
                }
            }
        }

        runtime
            .fibers()
            .shutdown()
            .expect("combined runtime should shut down fibers");
    }

    #[test]
    fn runtime_channel_service_can_run_on_managed_fiber() {
        let runtime = CurrentFiberAsyncBootstrap::uniform(
            1,
            NonZeroUsize::new(8 * 1024).expect("non-zero stack"),
            2,
        )
        .with_guard_pages(0)
        .build_current()
        .expect("combined runtime should build");

        {
            let expected = runtime
                .configured_memory_footprint()
                .expect("configured footprint should build");
            let service: CurrentFiberAsyncRuntimeChannelService<'_, 4, 4, 4> =
                CurrentFiberAsyncRuntimeChannelService::new(&runtime)
                    .expect("runtime channel service should build");
            let mut service = pin!(service);
            let mut stack_words = vec![0_u128; 2048].into_boxed_slice();
            let stack = FiberStack::from_slice(stack_words.as_mut()).expect("stack should build");
            let mut fiber = CurrentFiberAsyncRuntimeChannelService::spawn_managed::<8, 8>(
                service.as_mut(),
                stack,
            )
            .expect("managed runtime audit service fiber should build");

            let fiber_consumer = fiber
                .metadata_channel()
                .attach_consumer(TransportAttachmentRequest::same_courier())
                .expect("fiber metadata consumer should attach");
            let metadata_consumer = fiber
                .state()
                .metadata_channel()
                .attach_consumer(TransportAttachmentRequest::same_courier())
                .expect("metadata consumer should attach");
            let control_producer = fiber
                .state()
                .control_channel()
                .attach_producer(TransportAttachmentRequest::same_courier())
                .expect("control producer should attach");
            let status_consumer = fiber
                .state()
                .status_channel()
                .attach_consumer(TransportAttachmentRequest::same_courier())
                .expect("status consumer should attach");

            assert_eq!(
                fiber
                    .metadata_channel()
                    .try_receive(fiber_consumer)
                    .expect("fiber metadata receive should succeed"),
                Some(FiberMetadataMessage::Created { fiber: fiber.id() })
            );

            assert!(matches!(
                fiber.resume().expect("service fiber should yield"),
                FiberYield::Yielded
            ));

            assert_eq!(
                fiber
                    .metadata_channel()
                    .try_receive(fiber_consumer)
                    .expect("fiber metadata receive should succeed"),
                Some(FiberMetadataMessage::Started { fiber: fiber.id() })
            );

            let metadata = fiber
                .state()
                .metadata_channel()
                .try_receive(metadata_consumer)
                .expect("metadata receive should succeed")
                .expect("metadata should exist");
            match metadata {
                CurrentFiberAsyncRuntimeMetadataMessage::Advertised(info) => {
                    assert_eq!(info.executor_task_capacity, 2);
                    assert_eq!(info.fiber_carrier_count, 1);
                    assert!(info.fiber_task_capacity >= 1);
                }
            }

            fiber
                .state()
                .control_channel()
                .try_send(
                    control_producer,
                    CurrentFiberAsyncRuntimeControlRequest::ReadConfiguredMemoryFootprint,
                )
                .expect("configured footprint request should send");

            assert!(matches!(
                fiber.resume().expect("service fiber should yield"),
                FiberYield::Yielded
            ));

            let status = fiber
                .state()
                .status_channel()
                .try_receive(status_consumer)
                .expect("status receive should succeed")
                .expect("status should exist");
            match status {
                CurrentFiberAsyncRuntimeControlStatusMessage::ConfiguredMemoryFootprint(
                    footprint,
                ) => {
                    assert_eq!(footprint, expected);
                }
                other => panic!("unexpected runtime status: {other:?}"),
            }
        }

        runtime
            .fibers()
            .shutdown()
            .expect("combined runtime should shut down fibers");
    }

    #[cfg(feature = "debug-insights")]
    #[test]
    fn runtime_insight_skips_unobserved_payloads_and_emits_when_observed() {
        let runtime = CurrentFiberAsyncBootstrap::uniform(
            1,
            NonZeroUsize::new(8 * 1024).expect("non-zero stack"),
            2,
        )
        .with_guard_pages(0)
        .build_current()
        .expect("combined runtime should build");

        {
            let insight = CurrentFiberAsyncRuntimeInsight::<4, 4>::new(InsightCaptureMode::Lossy)
                .expect("runtime insight should build");

            assert!(
                !insight
                    .emit_state_if_observed(&runtime)
                    .expect("unobserved state emit should not fail")
            );
            assert!(
                !insight
                    .emit_configured_memory_footprint_if_observed(&runtime)
                    .expect("unobserved snapshot emit should not fail")
            );

            let state_consumer = insight
                .state_channel()
                .attach_consumer(TransportAttachmentRequest::same_courier())
                .expect("state consumer should attach");
            let snapshot_consumer = insight
                .snapshot_channel()
                .attach_consumer(TransportAttachmentRequest::same_courier())
                .expect("snapshot consumer should attach");

            assert!(
                insight
                    .emit_state_if_observed(&runtime)
                    .expect("observed state emit should succeed")
            );
            assert!(
                insight
                    .emit_configured_memory_footprint_if_observed(&runtime)
                    .expect("observed snapshot emit should succeed")
            );

            let state = insight
                .state_channel()
                .try_receive(state_consumer)
                .expect("state receive should succeed")
                .expect("state message should exist");
            assert_eq!(
                state,
                CurrentFiberAsyncRuntimeStateRecord::Configured(CurrentFiberAsyncRuntimeInfo {
                    fiber_carrier_count: 1,
                    fiber_task_capacity: runtime.fibers().memory_footprint().task_capacity,
                    executor_task_capacity: 2,
                })
            );

            let snapshot = insight
                .snapshot_channel()
                .try_receive(snapshot_consumer)
                .expect("snapshot receive should succeed")
                .expect("snapshot message should exist");
            assert_eq!(
                snapshot,
                CurrentFiberAsyncRuntimeSnapshotRecord::ConfiguredMemoryFootprint(
                    runtime
                        .configured_memory_footprint()
                        .expect("configured footprint should build")
                )
            );
        }

        runtime
            .fibers()
            .shutdown()
            .expect("combined runtime should shut down fibers");
    }
}
