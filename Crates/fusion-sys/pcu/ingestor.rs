//! Local PCU executor ingestor running on one proper fiber and speaking through channels.
//!
//! This is the first honest bridge between the PCU protocol vocabulary and the current local
//! execution substrate:
//! - metadata is surfaced on one read channel
//! - submissions are accepted on one write channel
//! - completion/status is surfaced on one read channel
//! - the service loop itself runs on one low-level `fusion-sys` fiber
//!
//! The important ownership rule here is deliberate:
//! - direct synchronous local dispatch may borrow caller-owned buffers
//! - channel/fiber submission must move the borrow-carrying binding object into the kernel path
//! - terminal status hands the binding object back so the caller regains the borrow honestly
//!
//! That keeps the local async path inside Rust's ownership model instead of hiding borrowed slices
//! behind raw-pointer registries and hoping nobody blinks at the wrong time.

use core::array;
use core::fmt;
use core::marker::PhantomData;
use core::pin::Pin;

use crate::channel::{ChannelError, ChannelErrorKind, ChannelReceive, ChannelSend, LocalChannel};
use crate::fiber::{
    Fiber,
    FiberError,
    FiberErrorKind,
    FiberReturn,
    FiberRunnable,
    FiberStack,
    FiberYield,
    ManagedFiber,
    yield_now,
};
use crate::protocol::{
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
use crate::transport::{
    TransportAttachmentControl,
    TransportAttachmentRequest,
    TransportDirection,
    TransportFraming,
};

use super::{
    PcuError,
    PcuErrorKind,
    PcuExecutorDescriptor,
    PcuExecutorId,
    PcuExecutorMetadataMessage,
    PcuExecutorMetadataProtocol,
    PcuInvocation,
    PcuInvocationBindings,
    PcuInvocationHandle,
    PcuInvocationParameters,
    PcuKernel,
    PcuKernelId,
    PcuParameterBinding,
    PcuSubmissionId,
    PcuSystem,
};

const ZERO_PARAMETER_BINDING: PcuParameterBinding = PcuParameterBinding {
    slot: fusion_pal::sys::pcu::PcuParameterSlot(0),
    value: fusion_pal::sys::pcu::PcuParameterValue::U32(0),
};

/// Fixed-capacity inline runtime-parameter payload for one local submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuLocalParameterBindings<const MAX_PARAMETERS: usize> {
    len: usize,
    bindings: [PcuParameterBinding; MAX_PARAMETERS],
}

impl<const MAX_PARAMETERS: usize> PcuLocalParameterBindings<MAX_PARAMETERS> {
    /// Returns one empty inline runtime-parameter payload.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            len: 0,
            bindings: [ZERO_PARAMETER_BINDING; MAX_PARAMETERS],
        }
    }

    /// Copies one borrowed runtime-parameter slice into this inline payload.
    ///
    /// # Errors
    ///
    /// Returns `ResourceExhausted` when the inline payload capacity is too small.
    pub fn from_slice(bindings: &[PcuParameterBinding]) -> Result<Self, PcuError> {
        if bindings.len() > MAX_PARAMETERS {
            return Err(PcuError::resource_exhausted());
        }
        let mut copied = Self::empty();
        copied.len = bindings.len();
        copied.bindings[..bindings.len()].copy_from_slice(bindings);
        Ok(copied)
    }

    /// Returns the truthful invocation-parameter view over this inline payload.
    #[must_use]
    pub fn as_invocation_parameters(&self) -> PcuInvocationParameters<'_> {
        PcuInvocationParameters {
            bindings: &self.bindings[..self.len],
        }
    }
}

/// Local submission request that carries ownership of one binding object into the ingestor.
#[derive(Debug)]
pub enum PcuLocalSubmissionRequest<'data, const MAX_PARAMETERS: usize> {
    Submit {
        submission: PcuSubmissionId,
        kernel: PcuKernelId,
        invocation: super::PcuInvocationShape,
        bindings: PcuInvocationBindings<'data>,
        parameters: PcuLocalParameterBindings<MAX_PARAMETERS>,
    },
    Cancel {
        submission: PcuSubmissionId,
    },
}

impl<'data, const MAX_PARAMETERS: usize> PcuLocalSubmissionRequest<'data, MAX_PARAMETERS> {
    /// Creates one ordinary local submission carrying bindings and optional runtime parameters.
    ///
    /// # Errors
    ///
    /// Returns `ResourceExhausted` when too many runtime parameters are supplied for the fixed
    /// inline capacity.
    pub fn submit(
        submission: PcuSubmissionId,
        kernel: PcuKernelId,
        invocation: super::PcuInvocationShape,
        bindings: PcuInvocationBindings<'data>,
        parameters: &[PcuParameterBinding],
    ) -> Result<Self, PcuError> {
        Ok(Self::Submit {
            submission,
            kernel,
            invocation,
            bindings,
            parameters: PcuLocalParameterBindings::from_slice(parameters)?,
        })
    }
}

/// Local submission status message that returns bindings on terminal completion.
#[derive(Debug)]
pub enum PcuLocalSubmissionStatusMessage<'data> {
    Accepted {
        submission: PcuSubmissionId,
        executor: PcuExecutorId,
    },
    Rejected {
        submission: PcuSubmissionId,
        reason: PcuErrorKind,
        bindings: Option<PcuInvocationBindings<'data>>,
    },
    Running {
        submission: PcuSubmissionId,
    },
    Completed {
        submission: PcuSubmissionId,
        bindings: PcuInvocationBindings<'data>,
    },
    Failed {
        submission: PcuSubmissionId,
        reason: PcuErrorKind,
        bindings: PcuInvocationBindings<'data>,
    },
    Cancelled {
        submission: PcuSubmissionId,
    },
}

/// Local submission/write protocol for the safe same-context ingestor path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuLocalSubmissionProtocol<'data, const MAX_PARAMETERS: usize>(PhantomData<&'data ()>);

impl<'data, const MAX_PARAMETERS: usize> Protocol
    for PcuLocalSubmissionProtocol<'data, MAX_PARAMETERS>
{
    type Message = PcuLocalSubmissionRequest<'data, MAX_PARAMETERS>;

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
        id: ProtocolId(0x4655_5349_4f4e_5043_555f_4c53_5542_0001),
        version: ProtocolVersion::new(1, 0, 0),
        caps: ProtocolCaps::empty(),
        bootstrap: ProtocolBootstrapKind::Immediate,
        debug_view: ProtocolDebugView::Structured,
        transport: ProtocolTransportRequirements {
            direction: TransportDirection::Unidirectional,
            framing: TransportFraming::Message,
            requires_ordering: true,
            requires_reliability: true,
            cross_courier_compatible: false,
            cross_domain_compatible: false,
        },
        implementation: ProtocolImplementationKind::Native,
    };
}

/// Local submission status/read protocol for the safe same-context ingestor path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuLocalSubmissionStatusProtocol<'data>(PhantomData<&'data ()>);

impl<'data> Protocol for PcuLocalSubmissionStatusProtocol<'data> {
    type Message = PcuLocalSubmissionStatusMessage<'data>;

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
        id: ProtocolId(0x4655_5349_4f4e_5043_555f_4c53_5441_0001),
        version: ProtocolVersion::new(1, 0, 0),
        caps: ProtocolCaps::empty(),
        bootstrap: ProtocolBootstrapKind::Immediate,
        debug_view: ProtocolDebugView::Structured,
        transport: ProtocolTransportRequirements {
            direction: TransportDirection::Unidirectional,
            framing: TransportFraming::Message,
            requires_ordering: true,
            requires_reliability: true,
            cross_courier_compatible: false,
            cross_domain_compatible: false,
        },
        implementation: ProtocolImplementationKind::Native,
    };
}

/// Error surfaced while constructing or driving one local PCU ingestor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcuIngestorError {
    kind: PcuIngestorErrorKind,
}

impl PcuIngestorError {
    /// Returns the concrete ingestor error kind.
    #[must_use]
    pub const fn kind(self) -> PcuIngestorErrorKind {
        self.kind
    }
}

/// Classification of local ingestor failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PcuIngestorErrorKind {
    Pcu(PcuErrorKind),
    Channel(ChannelErrorKind),
    Fiber(FiberErrorKind),
}

impl From<PcuError> for PcuIngestorError {
    fn from(value: PcuError) -> Self {
        Self {
            kind: PcuIngestorErrorKind::Pcu(value.kind()),
        }
    }
}

impl From<ChannelError> for PcuIngestorError {
    fn from(value: ChannelError) -> Self {
        Self {
            kind: PcuIngestorErrorKind::Channel(value.kind()),
        }
    }
}

impl From<FiberError> for PcuIngestorError {
    fn from(value: FiberError) -> Self {
        Self {
            kind: PcuIngestorErrorKind::Fiber(value.kind()),
        }
    }
}

impl From<fusion_pal::sys::transport::TransportError> for PcuIngestorError {
    fn from(value: fusion_pal::sys::transport::TransportError) -> Self {
        ChannelError::from(value).into()
    }
}

impl fmt::Display for PcuIngestorErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Pcu(kind) => write!(f, "pcu ingestor backend error: {kind}"),
            Self::Channel(kind) => write!(f, "pcu ingestor channel error: {kind}"),
            Self::Fiber(kind) => write!(f, "pcu ingestor fiber error: {kind}"),
        }
    }
}

impl fmt::Display for PcuIngestorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

/// Stable state owned by one local PCU executor ingestor.
pub struct PcuExecutorIngestorState<
    'kernel,
    'data,
    const MAX_KERNELS: usize,
    const MAX_PARAMETERS: usize,
    const SUBMISSION_CAPACITY: usize,
    const STATUS_CAPACITY: usize,
    const METADATA_CAPACITY: usize,
> {
    system: PcuSystem,
    executor: PcuExecutorDescriptor,
    kernels: [Option<PcuKernel<'kernel>>; MAX_KERNELS],
    metadata_published: bool,
    metadata_channel: LocalChannel<PcuExecutorMetadataProtocol, METADATA_CAPACITY>,
    submission_channel:
        LocalChannel<PcuLocalSubmissionProtocol<'data, MAX_PARAMETERS>, SUBMISSION_CAPACITY>,
    status_channel: LocalChannel<PcuLocalSubmissionStatusProtocol<'data>, STATUS_CAPACITY>,
    metadata_producer: usize,
    metadata_consumer: usize,
    submission_producer: usize,
    submission_consumer: usize,
    status_producer: usize,
    status_consumer: usize,
}

impl<
    'kernel,
    'data,
    const MAX_KERNELS: usize,
    const MAX_PARAMETERS: usize,
    const SUBMISSION_CAPACITY: usize,
    const STATUS_CAPACITY: usize,
    const METADATA_CAPACITY: usize,
>
    PcuExecutorIngestorState<
        'kernel,
        'data,
        MAX_KERNELS,
        MAX_PARAMETERS,
        SUBMISSION_CAPACITY,
        STATUS_CAPACITY,
        METADATA_CAPACITY,
    >
{
    /// Creates one local channel-backed ingestor state for one exact surfaced executor.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the executor is unknown or the channel endpoints cannot be
    /// attached.
    pub fn new(system: PcuSystem, executor: PcuExecutorId) -> Result<Self, PcuIngestorError> {
        let executor = *system.executor(executor).ok_or_else(PcuError::invalid)?;
        let metadata_channel =
            LocalChannel::<PcuExecutorMetadataProtocol, METADATA_CAPACITY>::new()?;
        let submission_channel = LocalChannel::<
            PcuLocalSubmissionProtocol<'data, MAX_PARAMETERS>,
            SUBMISSION_CAPACITY,
        >::new()?;
        let status_channel =
            LocalChannel::<PcuLocalSubmissionStatusProtocol<'data>, STATUS_CAPACITY>::new()?;
        let request = TransportAttachmentRequest::same_courier();

        let metadata_producer = metadata_channel.attach_producer(request)?;
        let metadata_consumer = metadata_channel.attach_consumer(request)?;
        let submission_producer = submission_channel.attach_producer(request)?;
        let submission_consumer = submission_channel.attach_consumer(request)?;
        let status_producer = status_channel.attach_producer(request)?;
        let status_consumer = status_channel.attach_consumer(request)?;

        Ok(Self {
            system,
            executor,
            kernels: array::from_fn(|_| None),
            metadata_published: false,
            metadata_channel,
            submission_channel,
            status_channel,
            metadata_producer,
            metadata_consumer,
            submission_producer,
            submission_consumer,
            status_producer,
            status_consumer,
        })
    }

    /// Returns the executor surfaced by this ingestor.
    #[must_use]
    pub const fn executor(&self) -> PcuExecutorDescriptor {
        self.executor
    }

    /// Returns one client view over the channel endpoints owned by this ingestor.
    #[must_use]
    pub const fn client(
        &self,
    ) -> PcuExecutorIngestorClient<
        '_,
        'kernel,
        'data,
        MAX_KERNELS,
        MAX_PARAMETERS,
        SUBMISSION_CAPACITY,
        STATUS_CAPACITY,
        METADATA_CAPACITY,
    > {
        PcuExecutorIngestorClient { state: self }
    }

    /// Registers one kernel under its stable kernel id.
    ///
    /// # Errors
    ///
    /// Returns `StateConflict` when the id is already present and `ResourceExhausted` when the
    /// fixed local registry is full.
    pub fn register_kernel(&mut self, kernel: PcuKernel<'kernel>) -> Result<PcuKernelId, PcuError> {
        let kernel_id = fusion_pal::sys::pcu::PcuKernelIr::id(&kernel);
        if self
            .kernels
            .iter()
            .flatten()
            .any(|existing| fusion_pal::sys::pcu::PcuKernelIr::id(existing) == kernel_id)
        {
            return Err(PcuError::state_conflict());
        }
        let slot = self
            .kernels
            .iter_mut()
            .find(|slot| slot.is_none())
            .ok_or_else(PcuError::resource_exhausted)?;
        *slot = Some(kernel);
        Ok(kernel_id)
    }

    fn kernel_by_id(&self, kernel_id: PcuKernelId) -> Option<PcuKernel<'kernel>> {
        self.kernels
            .iter()
            .flatten()
            .copied()
            .find(|kernel| fusion_pal::sys::pcu::PcuKernelIr::id(kernel) == kernel_id)
    }

    fn publish_metadata_if_needed(&mut self) -> bool {
        if self.metadata_published {
            return true;
        }
        match self.metadata_channel.try_send(
            self.metadata_producer,
            PcuExecutorMetadataMessage::Advertised(self.executor),
        ) {
            Ok(()) => {
                self.metadata_published = true;
                true
            }
            Err(_) => false,
        }
    }

    fn send_status(
        &self,
        message: PcuLocalSubmissionStatusMessage<'data>,
    ) -> Result<(), FiberError> {
        match self.status_channel.try_send(self.status_producer, message) {
            Ok(()) => Ok(()),
            Err(error) => Err(match error.kind() {
                ChannelErrorKind::Unsupported => FiberError::unsupported(),
                ChannelErrorKind::Invalid | ChannelErrorKind::ProtocolMismatch => {
                    FiberError::invalid()
                }
                ChannelErrorKind::Busy | ChannelErrorKind::ResourceExhausted => {
                    FiberError::resource_exhausted()
                }
                ChannelErrorKind::PermissionDenied
                | ChannelErrorKind::StateConflict
                | ChannelErrorKind::TransportDenied
                | ChannelErrorKind::Platform(_) => FiberError::state_conflict(),
            }),
        }
    }

    fn dispatch_bindings(
        system: PcuSystem,
        executor: PcuExecutorId,
        kernel: PcuKernel<'kernel>,
        invocation: super::PcuInvocationShape,
        bindings: &mut PcuInvocationBindings<'data>,
        parameters: PcuInvocationParameters<'_>,
    ) -> Result<(), PcuError> {
        let invocation = PcuInvocation {
            kernel: &kernel,
            shape: invocation,
        };

        let handle = match bindings {
            PcuInvocationBindings::StreamBytes(live) => system
                .dispatch_on_executor_with_parameters(
                    invocation,
                    PcuInvocationBindings::StreamBytes(super::PcuByteStreamBindings {
                        input: live.input,
                        output: live.output,
                    }),
                    parameters,
                    executor,
                )?,
            PcuInvocationBindings::StreamHalfWords(live) => system
                .dispatch_on_executor_with_parameters(
                    invocation,
                    PcuInvocationBindings::StreamHalfWords(super::PcuHalfWordStreamBindings {
                        input: live.input,
                        output: live.output,
                    }),
                    parameters,
                    executor,
                )?,
            PcuInvocationBindings::StreamWords(live) => system
                .dispatch_on_executor_with_parameters(
                    invocation,
                    PcuInvocationBindings::StreamWords(super::PcuWordStreamBindings {
                        input: live.input,
                        output: live.output,
                    }),
                    parameters,
                    executor,
                )?,
        };

        handle.wait()
    }

    fn handle_submission(
        &mut self,
        request: PcuLocalSubmissionRequest<'data, MAX_PARAMETERS>,
    ) -> Result<(), FiberError> {
        match request {
            PcuLocalSubmissionRequest::Cancel { submission } => {
                self.send_status(PcuLocalSubmissionStatusMessage::Rejected {
                    submission,
                    reason: PcuErrorKind::Unsupported,
                    bindings: None,
                })?;
            }
            PcuLocalSubmissionRequest::Submit {
                submission,
                kernel,
                invocation,
                mut bindings,
                parameters,
            } => {
                let Some(kernel) = self.kernel_by_id(kernel) else {
                    self.send_status(PcuLocalSubmissionStatusMessage::Rejected {
                        submission,
                        reason: PcuErrorKind::Invalid,
                        bindings: Some(bindings),
                    })?;
                    return Ok(());
                };

                let system = self.system;
                let executor = self.executor.id;
                self.send_status(PcuLocalSubmissionStatusMessage::Accepted {
                    submission,
                    executor,
                })?;
                self.send_status(PcuLocalSubmissionStatusMessage::Running { submission })?;

                match Self::dispatch_bindings(
                    system,
                    executor,
                    kernel,
                    invocation,
                    &mut bindings,
                    parameters.as_invocation_parameters(),
                ) {
                    Ok(()) => self.send_status(PcuLocalSubmissionStatusMessage::Completed {
                        submission,
                        bindings,
                    })?,
                    Err(error) => self.send_status(PcuLocalSubmissionStatusMessage::Failed {
                        submission,
                        reason: error.kind(),
                        bindings,
                    })?,
                }
            }
        }
        Ok(())
    }
}

/// Client-side view over one local executor ingestor's channels.
pub struct PcuExecutorIngestorClient<
    'state,
    'kernel,
    'data,
    const MAX_KERNELS: usize,
    const MAX_PARAMETERS: usize,
    const SUBMISSION_CAPACITY: usize,
    const STATUS_CAPACITY: usize,
    const METADATA_CAPACITY: usize,
> {
    state: &'state PcuExecutorIngestorState<
        'kernel,
        'data,
        MAX_KERNELS,
        MAX_PARAMETERS,
        SUBMISSION_CAPACITY,
        STATUS_CAPACITY,
        METADATA_CAPACITY,
    >,
}

impl<
    'state,
    'kernel,
    'data,
    const MAX_KERNELS: usize,
    const MAX_PARAMETERS: usize,
    const SUBMISSION_CAPACITY: usize,
    const STATUS_CAPACITY: usize,
    const METADATA_CAPACITY: usize,
>
    PcuExecutorIngestorClient<
        'state,
        'kernel,
        'data,
        MAX_KERNELS,
        MAX_PARAMETERS,
        SUBMISSION_CAPACITY,
        STATUS_CAPACITY,
        METADATA_CAPACITY,
    >
{
    /// Returns the metadata channel.
    #[must_use]
    pub const fn metadata_channel(
        &self,
    ) -> &LocalChannel<PcuExecutorMetadataProtocol, METADATA_CAPACITY> {
        &self.state.metadata_channel
    }

    /// Returns the local submission channel.
    #[must_use]
    pub const fn submission_channel(
        &self,
    ) -> &LocalChannel<PcuLocalSubmissionProtocol<'data, MAX_PARAMETERS>, SUBMISSION_CAPACITY> {
        &self.state.submission_channel
    }

    /// Returns the local status channel.
    #[must_use]
    pub const fn status_channel(
        &self,
    ) -> &LocalChannel<PcuLocalSubmissionStatusProtocol<'data>, STATUS_CAPACITY> {
        &self.state.status_channel
    }

    /// Receives one metadata message when available.
    pub fn try_receive_metadata(&self) -> Result<Option<PcuExecutorMetadataMessage>, ChannelError> {
        self.state
            .metadata_channel
            .try_receive(self.state.metadata_consumer)
    }

    /// Sends one local owned submission request into the ingestor.
    pub fn submit(
        &self,
        request: PcuLocalSubmissionRequest<'data, MAX_PARAMETERS>,
    ) -> Result<(), ChannelError> {
        self.state
            .submission_channel
            .try_send(self.state.submission_producer, request)
    }

    /// Receives one local status/completion message when available.
    pub fn try_receive_status(
        &self,
    ) -> Result<Option<PcuLocalSubmissionStatusMessage<'data>>, ChannelError> {
        self.state
            .status_channel
            .try_receive(self.state.status_consumer)
    }
}

/// One proper low-level fiber hosting a PCU executor ingestor loop.
pub struct PcuExecutorIngestor<
    'state,
    'kernel,
    'data,
    const MAX_KERNELS: usize,
    const MAX_PARAMETERS: usize,
    const SUBMISSION_CAPACITY: usize,
    const STATUS_CAPACITY: usize,
    const METADATA_CAPACITY: usize,
> {
    fiber: ManagedFiber<
        'state,
        PcuExecutorIngestorState<
            'kernel,
            'data,
            MAX_KERNELS,
            MAX_PARAMETERS,
            SUBMISSION_CAPACITY,
            STATUS_CAPACITY,
            METADATA_CAPACITY,
        >,
    >,
    _marker: PhantomData<
        &'state mut PcuExecutorIngestorState<
            'kernel,
            'data,
            MAX_KERNELS,
            MAX_PARAMETERS,
            SUBMISSION_CAPACITY,
            STATUS_CAPACITY,
            METADATA_CAPACITY,
        >,
    >,
}

impl<
    'state,
    'kernel,
    'data,
    const MAX_KERNELS: usize,
    const MAX_PARAMETERS: usize,
    const SUBMISSION_CAPACITY: usize,
    const STATUS_CAPACITY: usize,
    const METADATA_CAPACITY: usize,
>
    PcuExecutorIngestor<
        'state,
        'kernel,
        'data,
        MAX_KERNELS,
        MAX_PARAMETERS,
        SUBMISSION_CAPACITY,
        STATUS_CAPACITY,
        METADATA_CAPACITY,
    >
{
    /// Creates one low-level fiber-backed ingestor over caller-owned pinned state.
    ///
    /// # Errors
    ///
    /// Returns any honest fiber construction failure.
    pub fn new(
        state: Pin<
            &'state mut PcuExecutorIngestorState<
                'kernel,
                'data,
                MAX_KERNELS,
                MAX_PARAMETERS,
                SUBMISSION_CAPACITY,
                STATUS_CAPACITY,
                METADATA_CAPACITY,
            >,
        >,
        stack: FiberStack,
    ) -> Result<Self, PcuIngestorError> {
        let fiber = Fiber::spawn_managed(stack, state)?;
        Ok(Self {
            fiber,
            _marker: PhantomData,
        })
    }

    /// Returns one shared view of the pinned ingestor state.
    #[must_use]
    pub fn state(
        &self,
    ) -> &PcuExecutorIngestorState<
        'kernel,
        'data,
        MAX_KERNELS,
        MAX_PARAMETERS,
        SUBMISSION_CAPACITY,
        STATUS_CAPACITY,
        METADATA_CAPACITY,
    > {
        self.fiber.state()
    }

    /// Resumes the ingestor fiber once.
    ///
    /// # Errors
    ///
    /// Returns any honest low-level fiber resumption failure.
    pub fn pump(&mut self) -> Result<FiberYield, FiberError> {
        self.fiber.resume()
    }
}

impl<
    'kernel,
    'data,
    const MAX_KERNELS: usize,
    const MAX_PARAMETERS: usize,
    const SUBMISSION_CAPACITY: usize,
    const STATUS_CAPACITY: usize,
    const METADATA_CAPACITY: usize,
> FiberRunnable
    for PcuExecutorIngestorState<
        'kernel,
        'data,
        MAX_KERNELS,
        MAX_PARAMETERS,
        SUBMISSION_CAPACITY,
        STATUS_CAPACITY,
        METADATA_CAPACITY,
    >
{
    fn run(mut self: Pin<&mut Self>) -> FiberReturn {
        let state = self.as_mut().get_mut();

        loop {
            if !state.publish_metadata_if_needed() {
                if yield_now().is_err() {
                    return FiberReturn::new(1);
                }
                continue;
            }

            match state
                .submission_channel
                .try_receive(state.submission_consumer)
            {
                Ok(Some(request)) => {
                    if state.handle_submission(request).is_err() {
                        return FiberReturn::new(2);
                    }
                }
                Ok(None) => {}
                Err(_) => return FiberReturn::new(3),
            }

            if yield_now().is_err() {
                return FiberReturn::new(4);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use core::num::NonZeroU32;
    use core::pin::pin;

    use super::*;
    use crate::fiber::FiberState;
    use crate::pcu::{
        PcuParameterSlot,
        PcuParameterValue,
        PcuPort,
        PcuStreamCapabilities,
        PcuStreamKernelIr,
        PcuStreamPattern,
        PcuStreamValueType,
    };
    use crate::transport::TransportAttachmentControl;

    #[test]
    fn fiber_ingestor_executes_one_stream_submission() {
        let system = PcuSystem::new();
        let executor = system.executors()[0];
        let mut state = PcuExecutorIngestorState::<4, 4, 4, 8, 4>::new(system, executor.id)
            .expect("ingestor state should build");

        let ports = [
            PcuPort::stream_input(Some("input"), PcuStreamValueType::U32.as_value_type()),
            PcuPort::stream_output(Some("output"), PcuStreamValueType::U32.as_value_type()),
        ];
        let patterns = [PcuStreamPattern::BitReverse];
        let kernel = PcuKernel::Stream(PcuStreamKernelIr {
            id: PcuKernelId(0x51),
            entry_point: "bit_reverse",
            bindings: &[],
            ports: &ports,
            parameters: &[],
            patterns: &patterns,
            capabilities: PcuStreamCapabilities::FIFO_INPUT
                | PcuStreamCapabilities::FIFO_OUTPUT
                | PcuStreamCapabilities::BIT_REVERSE,
        });
        let kernel_id = state
            .register_kernel(kernel)
            .expect("kernel registration should succeed");

        let input = [0x0000_00f0_u32, 0x8000_0001];
        let mut output = [0_u32; 2];

        let mut stack_words = vec![0_u128; 4096].into_boxed_slice();
        let stack = FiberStack::from_slice(stack_words.as_mut()).expect("stack should be valid");

        let mut state = pin!(state);
        let mut ingestor =
            PcuExecutorIngestor::new(state.as_mut(), stack).expect("ingestor fiber should build");
        assert_eq!(ingestor.fiber.fiber_state(), FiberState::Created);

        assert!(matches!(
            ingestor.pump().expect("metadata pump should yield"),
            FiberYield::Yielded
        ));
        assert_eq!(
            ingestor
                .state()
                .client()
                .try_receive_metadata()
                .expect("metadata read should succeed"),
            Some(PcuExecutorMetadataMessage::Advertised(executor))
        );

        ingestor
            .state()
            .client()
            .submit(
                PcuLocalSubmissionRequest::submit(
                    super::super::PcuSubmissionId(9),
                    kernel_id,
                    super::super::PcuInvocationShape::threads(
                        NonZeroU32::new(1).expect("non-zero thread count"),
                    ),
                    PcuInvocationBindings::StreamWords(super::super::PcuWordStreamBindings {
                        input: &input,
                        output: &mut output,
                    }),
                    &[],
                )
                .expect("submission should build"),
            )
            .expect("submission should enqueue");

        assert!(matches!(
            ingestor.pump().expect("submission pump should yield"),
            FiberYield::Yielded
        ));

        let accepted = ingestor
            .state()
            .client()
            .try_receive_status()
            .expect("accepted status should read")
            .expect("accepted status should exist");
        assert!(matches!(
            accepted,
            PcuLocalSubmissionStatusMessage::Accepted {
                submission: super::super::PcuSubmissionId(9),
                executor: id,
            } if id == executor.id
        ));

        let running = ingestor
            .state()
            .client()
            .try_receive_status()
            .expect("running status should read")
            .expect("running status should exist");
        assert!(matches!(
            running,
            PcuLocalSubmissionStatusMessage::Running {
                submission: super::super::PcuSubmissionId(9),
            }
        ));

        let completed = ingestor
            .state()
            .client()
            .try_receive_status()
            .expect("completed status should read")
            .expect("completed status should exist");
        let PcuLocalSubmissionStatusMessage::Completed { bindings, .. } = completed else {
            panic!("expected completed status");
        };
        let PcuInvocationBindings::StreamWords(bindings) = bindings else {
            panic!("expected word-stream bindings");
        };
        assert_eq!(bindings.output, &[0x0f00_0000, 0x8000_0001]);
    }

    #[test]
    fn fiber_ingestor_executes_runtime_parameterized_submission() {
        let system = PcuSystem::new();
        let executor = system.executors()[0];
        let mut state = PcuExecutorIngestorState::<4, 4, 4, 8, 4>::new(system, executor.id)
            .expect("ingestor state should build");

        let ports = [
            PcuPort::stream_input(Some("input"), PcuStreamValueType::U32.as_value_type()),
            PcuPort::stream_output(Some("output"), PcuStreamValueType::U32.as_value_type()),
        ];
        let parameters = [fusion_pal::sys::pcu::PcuParameter::named(
            PcuParameterSlot(0),
            "mask",
            fusion_pal::sys::pcu::PcuValueType::u32(),
        )];
        let patterns = [PcuStreamPattern::XorParameter {
            parameter: PcuParameterSlot(0),
        }];
        let kernel = PcuKernel::Stream(PcuStreamKernelIr {
            id: PcuKernelId(0x52),
            entry_point: "xor_parameter",
            bindings: &[],
            ports: &ports,
            parameters: &parameters,
            patterns: &patterns,
            capabilities: PcuStreamCapabilities::FIFO_INPUT
                | PcuStreamCapabilities::FIFO_OUTPUT
                | PcuStreamCapabilities::XOR_PARAMETER,
        });
        let kernel_id = state
            .register_kernel(kernel)
            .expect("kernel registration should succeed");

        let input = [0x0000_ffff_u32, 0x1234_5678];
        let mut output = [0_u32; 2];
        let runtime_parameters = [PcuParameterBinding::new(
            PcuParameterSlot(0),
            PcuParameterValue::U32(0xffff_0000),
        )];

        let mut stack_words = vec![0_u128; 4096].into_boxed_slice();
        let stack = FiberStack::from_slice(stack_words.as_mut()).expect("stack should be valid");

        let mut state = pin!(state);
        let mut ingestor =
            PcuExecutorIngestor::new(state.as_mut(), stack).expect("ingestor fiber should build");

        assert!(matches!(
            ingestor.pump().expect("metadata pump should yield"),
            FiberYield::Yielded
        ));
        let _ = ingestor
            .state()
            .client()
            .try_receive_metadata()
            .expect("metadata read should succeed");

        ingestor
            .state()
            .client()
            .submit(
                PcuLocalSubmissionRequest::submit(
                    super::super::PcuSubmissionId(10),
                    kernel_id,
                    super::super::PcuInvocationShape::threads(
                        NonZeroU32::new(1).expect("non-zero thread count"),
                    ),
                    PcuInvocationBindings::StreamWords(super::super::PcuWordStreamBindings {
                        input: &input,
                        output: &mut output,
                    }),
                    &runtime_parameters,
                )
                .expect("parameterized submission should build"),
            )
            .expect("parameterized submission should enqueue");

        assert!(matches!(
            ingestor.pump().expect("submission pump should yield"),
            FiberYield::Yielded
        ));

        let _ = ingestor
            .state()
            .client()
            .try_receive_status()
            .expect("accepted status should read");
        let _ = ingestor
            .state()
            .client()
            .try_receive_status()
            .expect("running status should read");

        let completed = ingestor
            .state()
            .client()
            .try_receive_status()
            .expect("completed status should read")
            .expect("completed status should exist");
        let PcuLocalSubmissionStatusMessage::Completed { bindings, .. } = completed else {
            panic!("expected completed status");
        };
        let PcuInvocationBindings::StreamWords(bindings) = bindings else {
            panic!("expected word-stream bindings");
        };
        assert_eq!(bindings.output, &[0xffff_ffff, 0xedcb_5678]);
    }

    #[test]
    fn fiber_ingestor_faults_when_status_channel_is_no_longer_sendable() {
        let system = PcuSystem::new();
        let executor = system.executors()[0];
        let mut state = PcuExecutorIngestorState::<4, 4, 4, 8, 4>::new(system, executor.id)
            .expect("ingestor state should build");

        let ports = [
            PcuPort::stream_input(Some("input"), PcuStreamValueType::U32.as_value_type()),
            PcuPort::stream_output(Some("output"), PcuStreamValueType::U32.as_value_type()),
        ];
        let patterns = [PcuStreamPattern::BitReverse];
        let kernel = PcuKernel::Stream(PcuStreamKernelIr {
            id: PcuKernelId(0x53),
            entry_point: "bit_reverse",
            bindings: &[],
            ports: &ports,
            parameters: &[],
            patterns: &patterns,
            capabilities: PcuStreamCapabilities::FIFO_INPUT
                | PcuStreamCapabilities::FIFO_OUTPUT
                | PcuStreamCapabilities::BIT_REVERSE,
        });
        let kernel_id = state
            .register_kernel(kernel)
            .expect("kernel registration should succeed");

        let input = [0x0000_00f0_u32];
        let mut output = [0_u32; 1];

        let mut stack_words = vec![0_u128; 4096].into_boxed_slice();
        let stack = FiberStack::from_slice(stack_words.as_mut()).expect("stack should be valid");

        let mut state = pin!(state);
        let mut ingestor =
            PcuExecutorIngestor::new(state.as_mut(), stack).expect("ingestor fiber should build");

        assert!(matches!(
            ingestor.pump().expect("metadata pump should yield"),
            FiberYield::Yielded
        ));
        let _ = ingestor
            .state()
            .client()
            .try_receive_metadata()
            .expect("metadata read should succeed");

        ingestor
            .state()
            .status_channel
            .detach_producer(ingestor.state().status_producer)
            .expect("status producer should detach");

        ingestor
            .state()
            .client()
            .submit(
                PcuLocalSubmissionRequest::submit(
                    PcuSubmissionId(11),
                    kernel_id,
                    super::super::PcuInvocationShape::threads(
                        NonZeroU32::new(1).expect("non-zero thread count"),
                    ),
                    PcuInvocationBindings::StreamWords(super::super::PcuWordStreamBindings {
                        input: &input,
                        output: &mut output,
                    }),
                    &[],
                )
                .expect("submission should build"),
            )
            .expect("submission should enqueue");

        assert!(matches!(
            ingestor
                .pump()
                .expect("status failure should complete the fiber"),
            FiberYield::Completed(FiberReturn { code: 2 })
        ));
    }

    #[test]
    fn fiber_ingestor_rejects_cancel_requests_as_unsupported_today() {
        let system = PcuSystem::new();
        let executor = system.executors()[0];
        let state = PcuExecutorIngestorState::<4, 4, 4, 8, 4>::new(system, executor.id)
            .expect("ingestor state should build");

        let mut stack_words = vec![0_u128; 4096].into_boxed_slice();
        let stack = FiberStack::from_slice(stack_words.as_mut()).expect("stack should be valid");

        let mut state = pin!(state);
        let mut ingestor =
            PcuExecutorIngestor::new(state.as_mut(), stack).expect("ingestor fiber should build");

        assert!(matches!(
            ingestor.pump().expect("metadata pump should yield"),
            FiberYield::Yielded
        ));
        let _ = ingestor
            .state()
            .client()
            .try_receive_metadata()
            .expect("metadata read should succeed");

        ingestor
            .state()
            .client()
            .submit(PcuLocalSubmissionRequest::Cancel {
                submission: PcuSubmissionId(12),
            })
            .expect("cancel request should enqueue");

        assert!(matches!(
            ingestor.pump().expect("cancel pump should yield"),
            FiberYield::Yielded
        ));

        let status = ingestor
            .state()
            .client()
            .try_receive_status()
            .expect("cancel status should read")
            .expect("cancel status should exist");
        assert!(matches!(
            status,
            PcuLocalSubmissionStatusMessage::Rejected {
                submission: PcuSubmissionId(12),
                reason: PcuErrorKind::Unsupported,
                bindings: None,
            }
        ));
    }

    #[test]
    fn fiber_ingestor_processes_multiple_submissions_in_order() {
        let system = PcuSystem::new();
        let executor = system.executors()[0];
        let mut state = PcuExecutorIngestorState::<4, 4, 8, 8, 4>::new(system, executor.id)
            .expect("ingestor state should build");

        let ports = [
            PcuPort::stream_input(Some("input"), PcuStreamValueType::U32.as_value_type()),
            PcuPort::stream_output(Some("output"), PcuStreamValueType::U32.as_value_type()),
        ];
        let patterns = [PcuStreamPattern::Increment];
        let kernel = PcuKernel::Stream(PcuStreamKernelIr {
            id: PcuKernelId(0x54),
            entry_point: "increment",
            bindings: &[],
            ports: &ports,
            parameters: &[],
            patterns: &patterns,
            capabilities: PcuStreamCapabilities::FIFO_INPUT
                | PcuStreamCapabilities::FIFO_OUTPUT
                | PcuStreamCapabilities::INCREMENT,
        });
        let kernel_id = state
            .register_kernel(kernel)
            .expect("kernel registration should succeed");

        let input_a = [1_u32, 2];
        let input_b = [9_u32, 10];
        let mut output_a = [0_u32; 2];
        let mut output_b = [0_u32; 2];

        let mut stack_words = vec![0_u128; 4096].into_boxed_slice();
        let stack = FiberStack::from_slice(stack_words.as_mut()).expect("stack should be valid");

        let mut state = pin!(state);
        let mut ingestor =
            PcuExecutorIngestor::new(state.as_mut(), stack).expect("ingestor fiber should build");

        assert!(matches!(
            ingestor.pump().expect("metadata pump should yield"),
            FiberYield::Yielded
        ));
        let _ = ingestor
            .state()
            .client()
            .try_receive_metadata()
            .expect("metadata read should succeed");

        ingestor
            .state()
            .client()
            .submit(
                PcuLocalSubmissionRequest::submit(
                    PcuSubmissionId(13),
                    kernel_id,
                    super::super::PcuInvocationShape::threads(
                        NonZeroU32::new(1).expect("non-zero thread count"),
                    ),
                    PcuInvocationBindings::StreamWords(super::super::PcuWordStreamBindings {
                        input: &input_a,
                        output: &mut output_a,
                    }),
                    &[],
                )
                .expect("first submission should build"),
            )
            .expect("first submission should enqueue");
        ingestor
            .state()
            .client()
            .submit(
                PcuLocalSubmissionRequest::submit(
                    PcuSubmissionId(14),
                    kernel_id,
                    super::super::PcuInvocationShape::threads(
                        NonZeroU32::new(1).expect("non-zero thread count"),
                    ),
                    PcuInvocationBindings::StreamWords(super::super::PcuWordStreamBindings {
                        input: &input_b,
                        output: &mut output_b,
                    }),
                    &[],
                )
                .expect("second submission should build"),
            )
            .expect("second submission should enqueue");

        assert!(matches!(
            ingestor.pump().expect("first submission pump should yield"),
            FiberYield::Yielded
        ));
        assert!(matches!(
            ingestor
                .pump()
                .expect("second submission pump should yield"),
            FiberYield::Yielded
        ));

        let mut completed_bindings = [None, None];
        for slot in &mut completed_bindings {
            let _accepted = ingestor
                .state()
                .client()
                .try_receive_status()
                .expect("accepted status should read")
                .expect("accepted status should exist");
            let _running = ingestor
                .state()
                .client()
                .try_receive_status()
                .expect("running status should read")
                .expect("running status should exist");
            let completed = ingestor
                .state()
                .client()
                .try_receive_status()
                .expect("completed status should read")
                .expect("completed status should exist");
            let PcuLocalSubmissionStatusMessage::Completed { bindings, .. } = completed else {
                panic!("expected completed status");
            };
            *slot = Some(bindings);
        }

        let Some(PcuInvocationBindings::StreamWords(first_bindings)) = completed_bindings[0].take()
        else {
            panic!("expected first completed word bindings");
        };
        let Some(PcuInvocationBindings::StreamWords(second_bindings)) =
            completed_bindings[1].take()
        else {
            panic!("expected second completed word bindings");
        };
        assert_eq!(first_bindings.output, &[2, 3]);
        assert_eq!(second_bindings.output, &[10, 11]);
    }
}
