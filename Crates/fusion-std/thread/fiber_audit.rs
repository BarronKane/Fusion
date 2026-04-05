//! Channel-operable audit surface for current-thread fiber pools.
//!
//! Direct fiber-pool APIs remain first-class. This module exists so stack and footprint truth can
//! cross one honest protocol/transport/channel boundary when callers actually need that shape.

use core::pin::Pin;

use fusion_sys::channel::{
    ChannelError,
    ChannelErrorKind,
    ChannelReceiveContract,
    ChannelSendContract,
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
use fusion_sys::transport::protocol::{
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
use fusion_sys::transport::{
    TransportAttachmentControlContract,
    TransportAttachmentRequest,
    TransportDirection,
    TransportError,
    TransportErrorKind,
    TransportFraming,
};

use super::{
    CurrentFiberPool,
    FiberPlanningSupport,
    FiberPoolMemoryFootprint,
    FiberStackStats,
};

/// Public metadata surfaced over the current-thread fiber audit metadata channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CurrentFiberPoolInfo {
    /// Planning-time stack/context truth for the selected runtime lane.
    pub planning: FiberPlanningSupport,
    /// Number of carrier queues provisioned for this pool.
    pub carrier_count: usize,
    /// Total schedulable task slots across the pool.
    pub task_capacity: usize,
}

/// Metadata snapshot/event for one current-thread fiber pool audit surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CurrentFiberPoolMetadataMessage {
    Advertised(CurrentFiberPoolInfo),
}

/// Control request sent to one current-thread fiber pool audit service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CurrentFiberPoolControlRequest {
    ReadMemoryFootprint,
    ReadStackStats,
    RepublishMetadata,
}

/// Control/status message emitted by one current-thread fiber pool audit service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CurrentFiberPoolControlStatusMessage {
    MemoryFootprint(FiberPoolMemoryFootprint),
    StackStats(Option<FiberStackStats>),
    MetadataRepublishScheduled,
}

/// Metadata/read protocol for one current-thread fiber pool audit surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CurrentFiberPoolMetadataProtocol;

impl ProtocolContract for CurrentFiberPoolMetadataProtocol {
    type Message = CurrentFiberPoolMetadataMessage;

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
        id: ProtocolId(0x4655_5349_4f4e_4650_4350_5f4d_445f_0001),
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

/// Control/write protocol for one current-thread fiber pool audit surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CurrentFiberPoolControlWriteProtocol;

impl ProtocolContract for CurrentFiberPoolControlWriteProtocol {
    type Message = CurrentFiberPoolControlRequest;

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
        id: ProtocolId(0x4655_5349_4f4e_4650_4350_5f43_545f_0001),
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

/// Status/read protocol for one current-thread fiber pool audit surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CurrentFiberPoolControlStatusProtocol;

impl ProtocolContract for CurrentFiberPoolControlStatusProtocol {
    type Message = CurrentFiberPoolControlStatusMessage;

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
        id: ProtocolId(0x4655_5349_4f4e_4650_4350_5f53_545f_0001),
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

/// Error surfaced while constructing or pumping one current-thread fiber pool audit service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CurrentFiberPoolChannelServiceError {
    kind: CurrentFiberPoolChannelServiceErrorKind,
}

impl CurrentFiberPoolChannelServiceError {
    /// Returns the concrete service error kind.
    #[must_use]
    pub const fn kind(self) -> CurrentFiberPoolChannelServiceErrorKind {
        self.kind
    }
}

/// Classification of current-thread fiber pool audit service failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CurrentFiberPoolChannelServiceErrorKind {
    Channel(ChannelErrorKind),
    Transport(TransportErrorKind),
}

impl From<ChannelError> for CurrentFiberPoolChannelServiceError {
    fn from(value: ChannelError) -> Self {
        Self {
            kind: CurrentFiberPoolChannelServiceErrorKind::Channel(value.kind()),
        }
    }
}

impl From<TransportError> for CurrentFiberPoolChannelServiceError {
    fn from(value: TransportError) -> Self {
        Self {
            kind: CurrentFiberPoolChannelServiceErrorKind::Transport(value.kind()),
        }
    }
}

/// Same-context current-thread fiber pool audit service over local one-way channels.
pub struct CurrentFiberPoolChannelService<
    'a,
    const METADATA_CAPACITY: usize = 4,
    const CONTROL_CAPACITY: usize = 4,
    const STATUS_CAPACITY: usize = 4,
> {
    pool: &'a CurrentFiberPool,
    metadata_dirty: bool,
    pending_status: Option<CurrentFiberPoolControlStatusMessage>,
    metadata_channel: LocalChannel<CurrentFiberPoolMetadataProtocol, METADATA_CAPACITY>,
    control_channel: LocalChannel<CurrentFiberPoolControlWriteProtocol, CONTROL_CAPACITY>,
    status_channel: LocalChannel<CurrentFiberPoolControlStatusProtocol, STATUS_CAPACITY>,
    metadata_producer: usize,
    control_consumer: usize,
    status_producer: usize,
}

impl<
    'a,
    const METADATA_CAPACITY: usize,
    const CONTROL_CAPACITY: usize,
    const STATUS_CAPACITY: usize,
> CurrentFiberPoolChannelService<'a, METADATA_CAPACITY, CONTROL_CAPACITY, STATUS_CAPACITY>
{
    /// Creates one local current-thread fiber pool audit service.
    ///
    /// # Errors
    ///
    /// Returns an honest channel-transport failure when the local channels cannot be instantiated
    /// or attached.
    pub fn new(pool: &'a CurrentFiberPool) -> Result<Self, CurrentFiberPoolChannelServiceError> {
        let metadata_channel =
            LocalChannel::<CurrentFiberPoolMetadataProtocol, METADATA_CAPACITY>::new()?;
        let control_channel =
            LocalChannel::<CurrentFiberPoolControlWriteProtocol, CONTROL_CAPACITY>::new()?;
        let status_channel =
            LocalChannel::<CurrentFiberPoolControlStatusProtocol, STATUS_CAPACITY>::new()?;
        let request = TransportAttachmentRequest::same_courier();

        let metadata_producer = metadata_channel.attach_producer(request)?;
        let control_consumer = control_channel.attach_consumer(request)?;
        let status_producer = status_channel.attach_producer(request)?;

        Ok(Self {
            pool,
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

    /// Returns the fiber-pool metadata channel.
    #[must_use]
    pub const fn metadata_channel(
        &self,
    ) -> &LocalChannel<CurrentFiberPoolMetadataProtocol, METADATA_CAPACITY> {
        &self.metadata_channel
    }

    /// Returns the fiber-pool control channel.
    #[must_use]
    pub const fn control_channel(
        &self,
    ) -> &LocalChannel<CurrentFiberPoolControlWriteProtocol, CONTROL_CAPACITY> {
        &self.control_channel
    }

    /// Returns the fiber-pool status channel.
    #[must_use]
    pub const fn status_channel(
        &self,
    ) -> &LocalChannel<CurrentFiberPoolControlStatusProtocol, STATUS_CAPACITY> {
        &self.status_channel
    }

    /// Spawns this audit service on one managed fiber without any automatic fiber publication
    /// lane.
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

    /// Spawns this audit service on one managed fiber with one explicit opt-in publication lane.
    ///
    /// # Errors
    ///
    /// Returns any honest low-level fiber construction failure.
    pub fn spawn_managed_with_publication<
        'state,
        const META_FIBER_CAPACITY: usize,
        const MAX_CONSUMERS: usize,
    >(
        state: Pin<&'state mut Self>,
        stack: FiberStack,
    ) -> Result<ManagedFiber<'state, Self, META_FIBER_CAPACITY, MAX_CONSUMERS>, FiberError> {
        Fiber::spawn_managed_with_publication(stack, state)
    }

    /// Pumps pending metadata and control requests once.
    ///
    /// # Errors
    ///
    /// Returns an honest channel failure when the service cannot make forward progress honestly.
    pub fn pump(&mut self) -> Result<(), CurrentFiberPoolChannelServiceError> {
        self.flush_metadata()?;
        self.flush_pending_status()?;

        if self.pending_status.is_some() {
            return Ok(());
        }

        while let Some(request) = self.control_channel.try_receive(self.control_consumer)? {
            self.handle_request(request);
            self.flush_pending_status()?;
            self.flush_metadata()?;
            if self.pending_status.is_some() {
                break;
            }
        }

        Ok(())
    }

    fn flush_metadata(&mut self) -> Result<(), CurrentFiberPoolChannelServiceError> {
        if !self.metadata_dirty {
            return Ok(());
        }
        match self.metadata_channel.try_send(
            self.metadata_producer,
            CurrentFiberPoolMetadataMessage::Advertised(pool_info(self.pool)),
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

    fn flush_pending_status(&mut self) -> Result<(), CurrentFiberPoolChannelServiceError> {
        let Some(message) = self.pending_status.as_ref() else {
            return Ok(());
        };
        match self
            .status_channel
            .try_send(self.status_producer, message.clone())
        {
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
            Err(error) => {
                self.pending_status = None;
                Err(error.into())
            }
        }
    }

    fn handle_request(&mut self, request: CurrentFiberPoolControlRequest) {
        self.pending_status = Some(match request {
            CurrentFiberPoolControlRequest::ReadMemoryFootprint => {
                CurrentFiberPoolControlStatusMessage::MemoryFootprint(self.pool.memory_footprint())
            }
            CurrentFiberPoolControlRequest::ReadStackStats => {
                CurrentFiberPoolControlStatusMessage::StackStats(self.pool.stack_stats())
            }
            CurrentFiberPoolControlRequest::RepublishMetadata => {
                self.metadata_dirty = true;
                CurrentFiberPoolControlStatusMessage::MetadataRepublishScheduled
            }
        });
    }
}

impl<
    'a,
    const METADATA_CAPACITY: usize,
    const CONTROL_CAPACITY: usize,
    const STATUS_CAPACITY: usize,
> FiberRunnable
    for CurrentFiberPoolChannelService<'a, METADATA_CAPACITY, CONTROL_CAPACITY, STATUS_CAPACITY>
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

fn pool_info(pool: &CurrentFiberPool) -> CurrentFiberPoolInfo {
    let footprint = pool.memory_footprint();
    CurrentFiberPoolInfo {
        planning: FiberPlanningSupport::selected_runtime(),
        carrier_count: footprint.carrier_count,
        task_capacity: footprint.task_capacity,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thread::FiberPoolBootstrap;
    use core::pin::pin;
    use fusion_sys::fiber::{
        FiberMetadataMessage,
        FiberYield,
    };
    use std::vec;

    #[test]
    fn current_fiber_pool_channel_service_advertises_and_reports() {
        let fibers = FiberPoolBootstrap::fixed(2)
            .build_current()
            .expect("current fiber pool should build");

        let expected_footprint = fibers.memory_footprint();
        let expected_stats = fibers.stack_stats();
        let mut service: CurrentFiberPoolChannelService<'_, 4, 4, 4> =
            CurrentFiberPoolChannelService::new(&fibers)
                .expect("current fiber pool channel service should build");
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
            .expect("metadata should exist");
        match metadata {
            CurrentFiberPoolMetadataMessage::Advertised(info) => {
                assert_eq!(info.carrier_count, expected_footprint.carrier_count);
                assert_eq!(info.task_capacity, expected_footprint.task_capacity);
                assert_eq!(info.planning, FiberPlanningSupport::selected_runtime());
            }
        }

        service
            .control_channel()
            .try_send(
                control_producer,
                CurrentFiberPoolControlRequest::ReadMemoryFootprint,
            )
            .expect("memory footprint request should send");
        service
            .pump()
            .expect("service should handle memory footprint request");
        let status = service
            .status_channel()
            .try_receive(status_consumer)
            .expect("status receive should succeed")
            .expect("status should exist");
        assert_eq!(
            status,
            CurrentFiberPoolControlStatusMessage::MemoryFootprint(expected_footprint)
        );

        service
            .control_channel()
            .try_send(
                control_producer,
                CurrentFiberPoolControlRequest::ReadStackStats,
            )
            .expect("stack stats request should send");
        service
            .pump()
            .expect("service should handle stack stats request");
        let status = service
            .status_channel()
            .try_receive(status_consumer)
            .expect("status receive should succeed")
            .expect("status should exist");
        assert_eq!(
            status,
            CurrentFiberPoolControlStatusMessage::StackStats(expected_stats)
        );
    }

    #[test]
    fn current_fiber_pool_channel_service_can_run_on_managed_fiber() {
        let fibers = FiberPoolBootstrap::fixed(2)
            .build_current()
            .expect("current fiber pool should build");

        let expected_footprint = fibers.memory_footprint();
        let service: CurrentFiberPoolChannelService<'_, 4, 4, 4> =
            CurrentFiberPoolChannelService::new(&fibers)
                .expect("current fiber pool channel service should build");
        let mut service = pin!(service);
        let mut stack_words = vec![0_u128; 2048].into_boxed_slice();
        let stack = FiberStack::from_slice(stack_words.as_mut()).expect("stack should build");
        let mut fiber = CurrentFiberPoolChannelService::spawn_managed_with_publication::<8, 8>(
            service.as_mut(),
            stack,
        )
        .expect("managed current fiber pool service should build");

        let fiber_consumer = fiber
            .metadata_channel()
            .expect("managed audit fiber should expose explicit publication")
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
                .expect("managed audit fiber should expose explicit publication")
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
                .expect("managed audit fiber should expose explicit publication")
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
            CurrentFiberPoolMetadataMessage::Advertised(info) => {
                assert_eq!(info.carrier_count, expected_footprint.carrier_count);
                assert_eq!(info.task_capacity, expected_footprint.task_capacity);
                assert_eq!(info.planning, FiberPlanningSupport::selected_runtime());
            }
        }

        fiber
            .state()
            .control_channel()
            .try_send(
                control_producer,
                CurrentFiberPoolControlRequest::ReadMemoryFootprint,
            )
            .expect("memory footprint request should send");

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
        assert_eq!(
            status,
            CurrentFiberPoolControlStatusMessage::MemoryFootprint(expected_footprint)
        );
    }
}
