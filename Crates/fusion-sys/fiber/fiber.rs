//! fusion-sys-level stackful execution surfaces built on top of fusion-pal context switching.
//!
//! This layer stays deliberately narrow. It owns the low-level stackful primitive, the
//! carrier-local yield protocol, and the minimal bookkeeping required to let higher schedulers
//! resume and suspend fibers honestly. Scheduling policy stays above this layer.

use core::cell::UnsafeCell;
use core::fmt;
use core::marker::PhantomData;
use core::num::NonZeroUsize;
use core::pin::Pin;
use core::ptr::NonNull;
use core::sync::atomic::{
    AtomicU32,
    AtomicUsize,
    Ordering,
};

pub use fusion_pal::sys::execution_context::{
    ContextAuthoritySet,
    ContextBaseContract,
    ContextCaps,
    ContextError,
    ContextErrorKind,
    ContextGuarantee,
    ContextImplementationKind,
    ContextMigrationSupport,
    ContextStackDirection,
    ContextStackLayout,
    ContextSupport,
    ContextSwitch,
    ContextTlsIsolation,
    PlatformContext,
    PlatformSavedContext,
    RawContextEntry,
    system_context,
    system_context_support,
};

use crate::channel::{
    ChannelError,
    ChannelSendContract,
    LocalChannel,
};
use crate::claims::{
    ClaimAwareness,
    ClaimContextId,
};
use crate::domain::context::ContextId;
use crate::courier::CourierId;
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
use crate::sync::{
    OnceLock,
    SyncError,
    SyncErrorKind,
    ThinMutex,
};
use crate::thread::{
    ThreadErrorKind,
    ThreadId,
    ThreadSystem,
};
use crate::transport::{
    TransportAttachmentControlContract,
    TransportAttachmentLaw,
    TransportAttachmentRequest,
    TransportError,
    TransportErrorKind,
};

#[cfg(feature = "sys-cortex-m")]
const MAX_ACTIVE_FIBERS: usize = 8;
#[cfg(not(feature = "sys-cortex-m"))]
const MAX_ACTIVE_FIBERS: usize = 64;
#[cfg(feature = "sys-cortex-m")]
const MAX_FIBER_BOOTSTRAPS: usize = 32;
#[cfg(not(feature = "sys-cortex-m"))]
const MAX_FIBER_BOOTSTRAPS: usize = 256;

/// Low-level fiber support derived from the selected fusion-pal context backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FiberSupport {
    /// Raw context-switching support surfaced by the fusion-pal backend.
    pub context: ContextSupport,
}

/// Kind of fiber failure surfaced by `fusion-sys`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FiberErrorKind {
    /// Fiber switching is unsupported on the selected backend.
    Unsupported,
    /// The supplied stack or entry configuration was invalid.
    Invalid,
    /// Resources such as stack backing were exhausted.
    ResourceExhausted,
    /// The running fiber exceeded one declared execution budget.
    DeadlineExceeded,
    /// The requested operation conflicted with fiber state.
    StateConflict,
    /// The fusion-pal context backend reported a lower-level context failure.
    Context(ContextErrorKind),
}

/// Error surfaced by the low-level fiber layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FiberError {
    kind: FiberErrorKind,
}

impl FiberError {
    /// Creates an unsupported-operation error.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            kind: FiberErrorKind::Unsupported,
        }
    }

    /// Creates an invalid-configuration error.
    #[must_use]
    pub const fn invalid() -> Self {
        Self {
            kind: FiberErrorKind::Invalid,
        }
    }

    /// Creates a resource-exhaustion error.
    #[must_use]
    pub const fn resource_exhausted() -> Self {
        Self {
            kind: FiberErrorKind::ResourceExhausted,
        }
    }

    /// Creates one execution-budget overrun error.
    #[must_use]
    pub const fn deadline_exceeded() -> Self {
        Self {
            kind: FiberErrorKind::DeadlineExceeded,
        }
    }

    /// Creates a state-conflict error.
    #[must_use]
    pub const fn state_conflict() -> Self {
        Self {
            kind: FiberErrorKind::StateConflict,
        }
    }

    /// Returns the concrete fiber error kind.
    #[must_use]
    pub const fn kind(self) -> FiberErrorKind {
        self.kind
    }
}

impl From<ContextError> for FiberError {
    fn from(value: ContextError) -> Self {
        Self {
            kind: FiberErrorKind::Context(value.kind()),
        }
    }
}

impl fmt::Display for FiberErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Unsupported => f.write_str("fiber switching unsupported"),
            Self::Invalid => f.write_str("invalid fiber request"),
            Self::ResourceExhausted => f.write_str("fiber resources exhausted"),
            Self::DeadlineExceeded => f.write_str("fiber execution budget exceeded"),
            Self::StateConflict => f.write_str("fiber state conflict"),
            Self::Context(kind) => write!(f, "context backend error: {kind}"),
        }
    }
}

impl fmt::Display for FiberError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

/// Observable lifecycle state of a stackful fiber.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FiberState {
    /// The fiber has been created but never resumed.
    Created,
    /// The fiber is currently executing on a carrier.
    Running,
    /// The fiber yielded cooperatively and may resume later.
    Suspended,
    /// The fiber completed and will not resume again.
    Completed,
}

/// Logical return value produced by a fiber entry function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FiberReturn {
    /// Opaque completion code returned by the fiber.
    pub code: usize,
}

impl FiberReturn {
    /// Creates a new opaque fiber return record.
    #[must_use]
    pub const fn new(code: usize) -> Self {
        Self { code }
    }
}

/// Fiber entry signature used by the low-level stackful runtime.
pub type FiberEntry = unsafe fn(*mut ()) -> FiberReturn;

#[unsafe(no_mangle)]
pub static FUSION_FIBER_NEW_PHASE: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static FUSION_FIBER_BOOTSTRAP_PHASE: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static FUSION_FIBER_NEW_STACK_BASE_LOW: AtomicU32 = AtomicU32::new(0);
#[unsafe(no_mangle)]
pub static FUSION_FIBER_NEW_STACK_TOP_LOW: AtomicU32 = AtomicU32::new(0);

/// Safe typed entry contract for one pinned subsystem fiber state object.
pub trait FiberRunnable {
    /// Runs the fiber body on one pinned state object until it yields or completes.
    fn run(self: Pin<&mut Self>) -> FiberReturn;
}

/// Stable identifier for one managed Fusion fiber.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FiberId(usize);

impl FiberId {
    #[must_use]
    pub const fn new(value: usize) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

/// Coarse lifecycle metadata surfaced automatically by one managed fiber.
// TODO: narrow this lane toward "current situation" reporting once courier-owned ledgers become
// the authoritative substrate truth. The current event-shaped protocol is transitional only and
// should disappear or become fully opt-in once courier-owned metadata is the sole truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FiberMetadataMessage {
    /// One managed fiber was created and published one metadata event.
    Created { fiber: FiberId },
    /// One managed fiber started executing for the first time.
    Started { fiber: FiberId },
    /// One managed fiber completed with one terminal return value.
    Completed { fiber: FiberId, result: FiberReturn },
    /// One managed fiber failed to resume or otherwise faulted at the substrate boundary.
    Faulted {
        fiber: FiberId,
        reason: FiberErrorKind,
    },
    /// One managed fiber switched between claim-blind and black claim-aware execution.
    ClaimAwarenessChanged {
        fiber: FiberId,
        awareness: ClaimAwareness,
        claim_context: Option<ClaimContextId>,
    },
    /// One managed fiber was dropped before terminal completion.
    Abandoned {
        fiber: FiberId,
        lifecycle: FiberState,
    },
}

/// Managed-fiber metadata protocol carried over one transitional per-fiber publication channel.
pub struct FiberMetadataProtocol;

impl ProtocolContract for FiberMetadataProtocol {
    type Message = FiberMetadataMessage;

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor {
        id: ProtocolId(0x4655_5349_4f4e_5f46_4942_4552_4d44_0001),
        version: ProtocolVersion::new(1, 0, 0),
        caps: ProtocolCaps::DEBUG_VIEW,
        bootstrap: ProtocolBootstrapKind::Immediate,
        debug_view: ProtocolDebugView::Structured,
        transport: ProtocolTransportRequirements::message_local(),
        implementation: ProtocolImplementationKind::Native,
    };
}

/// Transitional publication channel carried by one managed fiber.
pub type FiberMetadataChannel<const CAPACITY: usize, const MAX_CONSUMERS: usize = 1> =
    LocalChannel<FiberMetadataProtocol, CAPACITY, MAX_CONSUMERS>;

/// One typed stackful fiber over caller-owned pinned state.
///
/// This quarantines the raw `*mut ()` trampoline inside `fusion-sys::fiber` so subsystem code
/// does not have to reimplement it every time it wants one managed fiber.
#[derive(Debug)]
pub struct PinnedFiber<'state, T: FiberRunnable> {
    state: NonNull<T>,
    fiber: Fiber,
    _marker: PhantomData<Pin<&'state mut T>>,
}

/// One subsystem-facing managed fiber with one transitional publication channel.
pub struct ManagedFiber<
    'state,
    T: FiberRunnable,
    const META_CAPACITY: usize = 16,
    const MAX_CONSUMERS: usize = 1,
> {
    id: FiberId,
    courier_id: Option<CourierId>,
    context_id: Option<ContextId>,
    started: bool,
    claim_awareness: ClaimAwareness,
    claim_context: Option<ClaimContextId>,
    fiber: Option<PinnedFiber<'state, T>>,
    metadata: Option<FiberMetadataChannel<META_CAPACITY, MAX_CONSUMERS>>,
    metadata_producer: Option<usize>,
}

/// Snapshot of one managed fiber's live identity, lifecycle, and claim-coupling state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ManagedFiberSnapshot {
    pub id: FiberId,
    pub state: FiberState,
    pub started: bool,
    pub claim_awareness: ClaimAwareness,
    pub claim_context: Option<ClaimContextId>,
}

impl ManagedFiberSnapshot {
    #[must_use]
    pub const fn is_running(self) -> bool {
        matches!(self.state, FiberState::Running)
    }

    #[must_use]
    pub const fn is_completed(self) -> bool {
        matches!(self.state, FiberState::Completed)
    }

    #[must_use]
    pub const fn is_claim_enabled(self) -> bool {
        self.claim_awareness.is_black() && self.claim_context.is_some()
    }
}

/// Yield outcome observed when resuming a fiber.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FiberYield {
    /// The fiber yielded cooperatively and may resume later.
    Yielded,
    /// The fiber completed and returned a terminal value.
    Completed(FiberReturn),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum FiberResumeOutcome {
    None,
    Yielded,
    Completed(FiberReturn),
}

/// Concrete stack reservation supplied to a fiber.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FiberStack {
    /// Base of the stack reservation.
    pub base: NonNull<u8>,
    /// Total bytes in the reservation.
    pub len: NonZeroUsize,
}

impl FiberStack {
    /// Creates one concrete stack reservation.
    ///
    /// # Errors
    ///
    /// Returns `invalid` when the supplied length is zero.
    pub fn new(base: NonNull<u8>, len: usize) -> Result<Self, FiberError> {
        Ok(Self {
            base,
            len: NonZeroUsize::new(len).ok_or_else(FiberError::invalid)?,
        })
    }

    /// Creates one concrete stack reservation from one live typed slice.
    ///
    /// # Errors
    ///
    /// Returns `invalid` when the supplied slice is empty.
    pub fn from_slice<T>(storage: &mut [T]) -> Result<Self, FiberError> {
        let base =
            NonNull::new(storage.as_mut_ptr().cast::<u8>()).ok_or_else(FiberError::invalid)?;
        Self::new(base, core::mem::size_of_val(storage))
    }
}

/// Stack request used when carving fiber stacks from a backing slab or pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FiberStackSpec {
    /// Requested stack size in bytes.
    pub size_bytes: NonZeroUsize,
    /// Requested guard size in bytes.
    pub guard_bytes: usize,
    /// Requested stack-backing behavior.
    pub backing: FiberStackBackingKind,
}

/// Low-level stack-backing mode requested for one fiber stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FiberStackBackingKind {
    /// Fully committed fixed-capacity stack.
    Fixed,
    /// Reservation-backed elastic stack with an initial committed prefix.
    Elastic {
        /// Initial committed usable bytes at stack creation.
        initial_usable_bytes: NonZeroUsize,
    },
}

/// fusion-sys context provider wrapper used by higher fiber layers.
#[derive(Debug, Clone, Copy)]
pub struct FiberSystem {
    inner: PlatformContext,
}

#[derive(Debug, Clone, Copy)]
struct FiberBootstrap {
    entry: Option<FiberEntry>,
    arg: *mut (),
    caller_context: *mut PlatformSavedContext,
    fiber_context: *mut PlatformSavedContext,
    outcome: *mut FiberResumeOutcome,
}

impl FiberBootstrap {
    const fn empty() -> Self {
        Self {
            entry: None,
            arg: core::ptr::null_mut(),
            caller_context: core::ptr::null_mut(),
            fiber_context: core::ptr::null_mut(),
            outcome: core::ptr::null_mut(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct FiberBootstrapSlot {
    allocated: bool,
    bootstrap: FiberBootstrap,
}

impl FiberBootstrapSlot {
    const fn empty() -> Self {
        Self {
            allocated: false,
            bootstrap: FiberBootstrap::empty(),
        }
    }
}

#[derive(Debug)]
struct FiberBootstrapRegistry {
    lock: ThinMutex,
    slots: UnsafeCell<[FiberBootstrapSlot; MAX_FIBER_BOOTSTRAPS]>,
}

impl FiberBootstrapRegistry {
    const fn new() -> Self {
        Self {
            lock: ThinMutex::new(),
            slots: UnsafeCell::new([FiberBootstrapSlot::empty(); MAX_FIBER_BOOTSTRAPS]),
        }
    }
}

// SAFETY: access to the mutable slot table is serialized through `lock`.
#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl Send for FiberBootstrapRegistry {}
// SAFETY: access to the mutable slot table is serialized through `lock`.
unsafe impl Sync for FiberBootstrapRegistry {}

#[derive(Debug, Clone, Copy)]
struct ActiveFiberSlot {
    active: bool,
    thread_id: ThreadId,
    fiber_id: Option<FiberId>,
    courier_id: Option<CourierId>,
    context_id: Option<ContextId>,
    arg: *mut (),
    caller_context: *mut PlatformSavedContext,
    fiber_context: *mut PlatformSavedContext,
    outcome: *mut FiberResumeOutcome,
}

impl ActiveFiberSlot {
    const fn empty() -> Self {
        Self {
            active: false,
            thread_id: ThreadId(0),
            fiber_id: None,
            courier_id: None,
            context_id: None,
            arg: core::ptr::null_mut(),
            caller_context: core::ptr::null_mut(),
            fiber_context: core::ptr::null_mut(),
            outcome: core::ptr::null_mut(),
        }
    }
}

#[derive(Debug)]
struct ActiveFiberRegistry {
    lock: ThinMutex,
    slots: UnsafeCell<[ActiveFiberSlot; MAX_ACTIVE_FIBERS]>,
}

impl ActiveFiberRegistry {
    const fn new() -> Self {
        Self {
            lock: ThinMutex::new(),
            slots: UnsafeCell::new([ActiveFiberSlot::empty(); MAX_ACTIVE_FIBERS]),
        }
    }
}

// SAFETY: access to the mutable slot table is serialized through `lock`.
#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl Send for ActiveFiberRegistry {}
// SAFETY: access to the mutable slot table is serialized through `lock`.
unsafe impl Sync for ActiveFiberRegistry {}

/// Low-level stackful fiber primitive.
#[derive(Debug)]
pub struct Fiber {
    context: PlatformSavedContext,
    bootstrap_slot: Option<usize>,
    outcome: FiberResumeOutcome,
    stack: FiberStack,
    state: FiberState,
}

// SAFETY: `Fiber` contains raw context pointers that are only dereferenced while the caller
// holds exclusive `&mut self` access during `resume()`. The primitive relies on the selected
// fusion-pal backend to enforce whatever migration contract it reports at runtime.
unsafe impl Send for Fiber {}

impl FiberSystem {
    /// Creates a wrapper for the selected platform context backend.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: system_context(),
        }
    }

    /// Reports the truthful context-switch surface available to fibers.
    #[must_use]
    pub fn support(&self) -> FiberSupport {
        FiberSupport {
            context: ContextBaseContract::support(&self.inner),
        }
    }
}

impl Default for FiberSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl<'state, T: FiberRunnable> PinnedFiber<'state, T> {
    /// Creates one typed stackful fiber over caller-owned pinned state.
    ///
    /// # Errors
    ///
    /// Returns any honest low-level fiber construction failure.
    pub fn new(state: Pin<&'state mut T>, stack: FiberStack) -> Result<Self, FiberError> {
        let state_ptr = NonNull::from(state.as_ref().get_ref());
        let fiber = Fiber::new(stack, pinned_fiber_entry::<T>, state_ptr.as_ptr().cast())?;
        Ok(Self {
            state: state_ptr,
            fiber,
            _marker: PhantomData,
        })
    }

    /// Returns one shared view of the pinned fiber state.
    #[must_use]
    pub fn state(&self) -> &T {
        // SAFETY: the state pointer comes from one live pinned reference held for `'state`.
        unsafe { self.state.as_ref() }
    }

    /// Returns one pinned mutable view of the fiber state.
    #[must_use]
    pub fn state_mut(&mut self) -> Pin<&mut T> {
        // SAFETY: the state remains pinned for `'state` and `&mut self` guarantees exclusivity.
        unsafe { Pin::new_unchecked(self.state.as_mut()) }
    }

    /// Returns the lifecycle state of the underlying low-level fiber.
    #[must_use]
    pub fn fiber_state(&self) -> FiberState {
        self.fiber.state()
    }

    /// Resumes the underlying fiber once.
    ///
    /// # Errors
    ///
    /// Returns any honest low-level fiber resumption failure.
    pub fn resume(&mut self) -> Result<FiberYield, FiberError> {
        self.fiber.resume()
    }

    fn resume_with_active_id(
        &mut self,
        fiber_id: Option<FiberId>,
        courier_id: Option<CourierId>,
        context_id: Option<ContextId>,
    ) -> Result<FiberYield, FiberError> {
        self.fiber
            .resume_with_active_id(fiber_id, courier_id, context_id)
    }

    /// Returns the owned stack reservation once the fiber has completed.
    ///
    /// # Errors
    ///
    /// Returns a state-conflict error when the fiber has not completed yet.
    pub fn into_stack(self) -> Result<FiberStack, FiberError> {
        self.fiber.into_stack()
    }
}

impl<'state, T: FiberRunnable, const META_CAPACITY: usize, const MAX_CONSUMERS: usize>
    ManagedFiber<'state, T, META_CAPACITY, MAX_CONSUMERS>
{
    /// Creates one managed fiber without any automatic publication channel.
    ///
    /// # Errors
    ///
    /// Returns any honest fiber construction failure.
    pub fn new(state: Pin<&'state mut T>, stack: FiberStack) -> Result<Self, FiberError> {
        let fiber = PinnedFiber::new(state, stack)?;
        Ok(Self {
            id: next_fiber_id(),
            courier_id: None,
            context_id: None,
            started: false,
            claim_awareness: ClaimAwareness::Blind,
            claim_context: None,
            fiber: Some(fiber),
            metadata: None,
            metadata_producer: None,
        })
    }

    /// Creates one managed fiber with one explicit opt-in publication channel.
    ///
    /// # Errors
    ///
    /// Returns any honest fiber or metadata-channel construction failure.
    pub fn new_with_publication(
        state: Pin<&'state mut T>,
        stack: FiberStack,
    ) -> Result<Self, FiberError> {
        let fiber = PinnedFiber::new(state, stack)?;
        let metadata =
            FiberMetadataChannel::<META_CAPACITY, MAX_CONSUMERS>::new_with_attachment_law(
                TransportAttachmentLaw::ExclusiveSpsc,
            )
            .map_err(fiber_error_from_channel)?;
        let metadata_producer = metadata
            .attach_producer(TransportAttachmentRequest::same_courier())
            .map_err(fiber_error_from_transport)?;
        let managed = Self {
            id: next_fiber_id(),
            courier_id: None,
            context_id: None,
            started: false,
            claim_awareness: ClaimAwareness::Blind,
            claim_context: None,
            fiber: Some(fiber),
            metadata: Some(metadata),
            metadata_producer: Some(metadata_producer),
        };
        managed.publish_metadata(FiberMetadataMessage::Created { fiber: managed.id });
        Ok(managed)
    }

    /// Returns the stable managed-fiber identifier.
    #[must_use]
    pub const fn id(&self) -> FiberId {
        self.id
    }

    /// Returns the courier this fiber is currently admitted under, when known.
    ///
    /// Lower layers do not invent courier identity on their own. Until a real courier admission
    /// path binds this fiber, the answer stays `None` instead of hallucinating one.
    #[must_use]
    pub const fn courier_id(&self) -> Option<CourierId> {
        self.courier_id
    }

    /// Returns the context this fiber is currently admitted under, when known.
    #[must_use]
    pub const fn context_id(&self) -> Option<ContextId> {
        self.context_id
    }

    /// Binds this managed fiber to one courier identity for runtime self-query surfaces.
    ///
    /// This is a transitional low-level hook. The real courier runtime should set this
    /// automatically at admission time once the scheduler/courier boundary owns that truth.
    pub fn bind_to_courier(&mut self, courier_id: CourierId) {
        self.courier_id = Some(courier_id);
    }

    /// Binds this managed fiber to one context identity for runtime self-query surfaces.
    pub fn bind_to_context(&mut self, context_id: ContextId) {
        self.context_id = Some(context_id);
    }

    /// Returns whether the managed fiber has ever been resumed.
    #[must_use]
    pub const fn started(&self) -> bool {
        self.started
    }

    /// Returns whether this fiber is currently claim-blind or black/claim-enabled.
    #[must_use]
    pub const fn claim_awareness(&self) -> ClaimAwareness {
        self.claim_awareness
    }

    /// Returns the active claim-context identifier carried by this fiber, if any.
    #[must_use]
    pub const fn claim_context(&self) -> Option<ClaimContextId> {
        self.claim_context
    }

    /// Returns whether this managed fiber is currently claim-enabled.
    #[must_use]
    pub const fn is_claim_enabled(&self) -> bool {
        self.claim_awareness.is_black() && self.claim_context.is_some()
    }

    /// Switches this fiber into one explicit claim mode.
    ///
    /// # Errors
    ///
    /// Returns `invalid` when black claim-aware mode is requested without one claim-context ID.
    pub fn set_claim_mode(
        &mut self,
        awareness: ClaimAwareness,
        claim_context: Option<ClaimContextId>,
    ) -> Result<(), FiberError> {
        if awareness.is_black() && claim_context.is_none() {
            return Err(FiberError::invalid());
        }
        self.claim_awareness = awareness;
        self.claim_context = claim_context;
        self.publish_metadata(FiberMetadataMessage::ClaimAwarenessChanged {
            fiber: self.id,
            awareness,
            claim_context,
        });
        Ok(())
    }

    /// Returns the optional explicit publication channel owned by this fiber.
    #[must_use]
    pub const fn metadata_channel(
        &self,
    ) -> Option<&FiberMetadataChannel<META_CAPACITY, MAX_CONSUMERS>> {
        self.metadata.as_ref()
    }

    /// Returns one shared view of the pinned fiber state.
    #[must_use]
    pub fn state(&self) -> &T {
        self.fiber
            .as_ref()
            .expect("managed fiber state should remain present until stack extraction")
            .state()
    }

    /// Returns one pinned mutable view of the managed fiber state.
    #[must_use]
    pub fn state_mut(&mut self) -> Pin<&mut T> {
        self.fiber
            .as_mut()
            .expect("managed fiber state should remain present until stack extraction")
            .state_mut()
    }

    /// Returns the lifecycle state of the underlying low-level fiber.
    #[must_use]
    pub fn fiber_state(&self) -> FiberState {
        self.fiber
            .as_ref()
            .expect("managed fiber state should remain present until stack extraction")
            .fiber_state()
    }

    /// Returns a stable snapshot of the managed fiber's current execution and claim state.
    #[must_use]
    pub fn snapshot(&self) -> ManagedFiberSnapshot {
        ManagedFiberSnapshot {
            id: self.id,
            state: self.fiber_state(),
            started: self.started,
            claim_awareness: self.claim_awareness,
            claim_context: self.claim_context,
        }
    }

    /// Returns the managed fiber's current situation for courier-facing supervision.
    #[must_use]
    pub fn current_situation(&self) -> ManagedFiberSnapshot {
        self.snapshot()
    }

    /// Returns whether the managed fiber is actively running right now.
    #[must_use]
    pub fn is_running(&self) -> bool {
        matches!(self.fiber_state(), FiberState::Running)
    }

    /// Returns whether the managed fiber has completed permanently.
    #[must_use]
    pub fn is_completed(&self) -> bool {
        matches!(self.fiber_state(), FiberState::Completed)
    }

    /// Resumes the managed fiber once and emits coarse lifecycle metadata.
    ///
    /// # Errors
    ///
    /// Returns any honest low-level fiber resumption failure.
    pub fn resume(&mut self) -> Result<FiberYield, FiberError> {
        if !self.started {
            self.publish_metadata(FiberMetadataMessage::Started { fiber: self.id });
            self.started = true;
        }

        match self
            .fiber
            .as_mut()
            .expect("managed fiber state should remain present until stack extraction")
            .resume_with_active_id(Some(self.id), self.courier_id, self.context_id)
        {
            Ok(FiberYield::Yielded) => Ok(FiberYield::Yielded),
            Ok(FiberYield::Completed(result)) => {
                self.publish_metadata(FiberMetadataMessage::Completed {
                    fiber: self.id,
                    result,
                });
                Ok(FiberYield::Completed(result))
            }
            Err(error) => {
                self.publish_metadata(FiberMetadataMessage::Faulted {
                    fiber: self.id,
                    reason: error.kind(),
                });
                Err(error)
            }
        }
    }

    /// Returns the owned stack reservation once the managed fiber has completed.
    ///
    /// # Errors
    ///
    /// Returns a state-conflict error when the fiber has not completed yet.
    pub fn into_stack(mut self) -> Result<FiberStack, FiberError> {
        self.fiber
            .take()
            .expect("managed fiber should still own one pinned fiber at stack extraction")
            .into_stack()
    }

    fn publish_metadata(&self, message: FiberMetadataMessage) {
        let Some(metadata) = self.metadata.as_ref() else {
            return;
        };
        let Some(producer) = self.metadata_producer else {
            return;
        };
        let _ = metadata.try_send(producer, message);
    }
}

impl<'state, T: FiberRunnable, const META_CAPACITY: usize, const MAX_CONSUMERS: usize> Drop
    for ManagedFiber<'state, T, META_CAPACITY, MAX_CONSUMERS>
{
    fn drop(&mut self) {
        if let Some(fiber) = self.fiber.as_ref() {
            let lifecycle = fiber.fiber_state();
            if lifecycle != FiberState::Completed {
                self.publish_metadata(FiberMetadataMessage::Abandoned {
                    fiber: self.id,
                    lifecycle,
                });
            }
        }
    }
}

impl Fiber {
    /// Creates a low-level fiber on the supplied stack.
    ///
    /// # Errors
    ///
    /// Returns an error when the selected backend cannot honestly construct a stackful
    /// execution context.
    pub fn new(stack: FiberStack, entry: FiberEntry, arg: *mut ()) -> Result<Self, FiberError> {
        FUSION_FIBER_NEW_PHASE.store(1, Ordering::Release);
        FUSION_FIBER_NEW_STACK_BASE_LOW.store(
            (stack.base.as_ptr() as usize & 0xffff_ffff) as u32,
            Ordering::Release,
        );
        FUSION_FIBER_NEW_STACK_TOP_LOW.store(
            ((stack.base.as_ptr() as usize).saturating_add(stack.len.get()) & 0xffff_ffff) as u32,
            Ordering::Release,
        );
        let context = system_context();
        let support = context.support();
        if !support.caps.contains(ContextCaps::MAKE) || !support.caps.contains(ContextCaps::SWAP) {
            return Err(FiberError::unsupported());
        }
        FUSION_FIBER_NEW_PHASE.store(2, Ordering::Release);

        let mut fiber = Self {
            context: PlatformSavedContext::default(),
            bootstrap_slot: None,
            outcome: FiberResumeOutcome::None,
            stack,
            state: FiberState::Created,
        };

        let bootstrap_slot = allocate_bootstrap(entry, arg)?;
        FUSION_FIBER_NEW_PHASE.store(3, Ordering::Release);
        let stack_layout = ContextStackLayout {
            base: stack.base,
            len: stack.len,
        };
        let bootstrap_ptr = with_bootstrap(bootstrap_slot, |bootstrap| {
            Ok(core::ptr::from_mut(bootstrap).cast())
        })?;
        FUSION_FIBER_NEW_PHASE.store(4, Ordering::Release);
        fiber.context =
            match unsafe { context.make(stack_layout, fiber_entry_trampoline, bootstrap_ptr) } {
                Ok(saved) => saved,
                Err(error) => {
                    release_bootstrap(bootstrap_slot)?;
                    return Err(FiberError::from(error));
                }
            };
        FUSION_FIBER_NEW_PHASE.store(5, Ordering::Release);
        fiber.bootstrap_slot = Some(bootstrap_slot);
        FUSION_FIBER_NEW_PHASE.store(6, Ordering::Release);
        Ok(fiber)
    }

    /// Creates one typed stackful fiber over caller-owned pinned state.
    ///
    /// This is the safe subsystem-facing bootstrap above the raw `FiberEntry(*mut ())` ABI.
    ///
    /// # Errors
    ///
    /// Returns any honest low-level fiber construction failure.
    pub fn spawn_pinned<'state, T: FiberRunnable>(
        stack: FiberStack,
        state: Pin<&'state mut T>,
    ) -> Result<PinnedFiber<'state, T>, FiberError> {
        PinnedFiber::new(state, stack)
    }

    /// Creates one managed typed stackful fiber without any automatic publication channel.
    ///
    /// # Errors
    ///
    /// Returns any honest low-level fiber construction failure.
    pub fn spawn_managed<
        'state,
        T: FiberRunnable,
        const META_CAPACITY: usize,
        const MAX_CONSUMERS: usize,
    >(
        stack: FiberStack,
        state: Pin<&'state mut T>,
    ) -> Result<ManagedFiber<'state, T, META_CAPACITY, MAX_CONSUMERS>, FiberError> {
        ManagedFiber::new(state, stack)
    }

    /// Creates one managed typed stackful fiber with one explicit opt-in publication channel.
    ///
    /// # Errors
    ///
    /// Returns any honest low-level fiber or metadata-channel construction failure.
    pub fn spawn_managed_with_publication<
        'state,
        T: FiberRunnable,
        const META_CAPACITY: usize,
        const MAX_CONSUMERS: usize,
    >(
        stack: FiberStack,
        state: Pin<&'state mut T>,
    ) -> Result<ManagedFiber<'state, T, META_CAPACITY, MAX_CONSUMERS>, FiberError> {
        ManagedFiber::new_with_publication(state, stack)
    }

    /// Resumes the fiber on the current carrier thread.
    ///
    /// # Errors
    ///
    /// Returns an error if the fiber is already running or has completed, or the selected
    /// backend cannot resume the saved context honestly.
    pub fn resume(&mut self) -> Result<FiberYield, FiberError> {
        self.resume_with_active_id(None, None, None)
    }

    /// Resumes this fiber while binding one explicit active execution identity.
    ///
    /// Scheduler layers that already know the owning fiber/courier/context should use this
    /// instead of hoping higher introspection surfaces can rediscover that truth after the fact.
    pub fn resume_bound(
        &mut self,
        fiber_id: Option<FiberId>,
        courier_id: Option<CourierId>,
        context_id: Option<ContextId>,
    ) -> Result<FiberYield, FiberError> {
        self.resume_with_active_id(fiber_id, courier_id, context_id)
    }

    fn resume_with_active_id(
        &mut self,
        fiber_id: Option<FiberId>,
        courier_id: Option<CourierId>,
        context_id: Option<ContextId>,
    ) -> Result<FiberYield, FiberError> {
        if matches!(self.state, FiberState::Running | FiberState::Completed) {
            return Err(FiberError::state_conflict());
        }

        let context = system_context();
        let bootstrap_slot = self.bootstrap_slot.ok_or_else(FiberError::state_conflict)?;
        let mut caller = PlatformSavedContext::default();
        self.outcome = FiberResumeOutcome::None;
        let caller_context = &raw mut caller;
        let fiber_context = &raw mut self.context;
        let outcome = &raw mut self.outcome;
        with_bootstrap(bootstrap_slot, |bootstrap| {
            bootstrap.caller_context = caller_context;
            bootstrap.fiber_context = fiber_context;
            bootstrap.outcome = outcome;
            Ok(())
        })?;
        let active_arg = with_bootstrap(bootstrap_slot, |bootstrap| Ok(bootstrap.arg))?;

        install_active_fiber(ActiveFiberSlot {
            active: true,
            thread_id: current_thread_id()?,
            fiber_id,
            courier_id,
            context_id,
            arg: active_arg,
            caller_context,
            fiber_context,
            outcome,
        })?;

        self.state = FiberState::Running;
        let swap_result =
            unsafe { context.swap(&mut caller, &self.context) }.map_err(FiberError::from);
        let clear_result = clear_active_fiber();

        if let Err(error) = swap_result {
            self.state = FiberState::Suspended;
            let _ = clear_result;
            return Err(error);
        }
        clear_result?;

        match self.outcome {
            FiberResumeOutcome::Yielded => {
                self.state = FiberState::Suspended;
                Ok(FiberYield::Yielded)
            }
            FiberResumeOutcome::Completed(result) => {
                self.state = FiberState::Completed;
                Ok(FiberYield::Completed(result))
            }
            FiberResumeOutcome::None => {
                self.state = FiberState::Suspended;
                Err(FiberError::state_conflict())
            }
        }
    }

    /// Returns the current lifecycle state of the fiber.
    #[must_use]
    pub const fn state(&self) -> FiberState {
        self.state
    }

    /// Returns the owned stack reservation once the fiber has completed.
    ///
    /// # Errors
    ///
    /// Returns a state-conflict error when the fiber has not completed yet.
    pub fn into_stack(self) -> Result<FiberStack, FiberError> {
        if self.state != FiberState::Completed {
            return Err(FiberError::state_conflict());
        }
        Ok(self.stack)
    }
}

impl Drop for Fiber {
    fn drop(&mut self) {
        if let Some(slot_index) = self.bootstrap_slot.take() {
            let _ = release_bootstrap(slot_index);
        }
    }
}

/// Yields the currently running fiber back to its carrier-side caller.
///
/// # Errors
///
/// Returns an error if no active fiber is registered on the current carrier or the selected
/// backend cannot perform the context switch honestly.
pub fn yield_now() -> Result<(), FiberError> {
    let active = current_active_fiber()?;
    if active.caller_context.is_null() || active.fiber_context.is_null() || active.outcome.is_null()
    {
        return Err(FiberError::state_conflict());
    }

    unsafe {
        *active.outcome = FiberResumeOutcome::Yielded;
    }

    let context = system_context();
    unsafe { context.swap(&mut *active.fiber_context, &*active.caller_context)? };
    Ok(())
}

/// Returns the current fiber's opaque caller-provided context pointer.
///
/// # Errors
///
/// Returns an error if no active fiber is registered on the current carrier.
pub fn current_context() -> Result<*mut (), FiberError> {
    let active = current_active_fiber()?;
    if active.arg.is_null() {
        return Err(FiberError::state_conflict());
    }
    Ok(active.arg)
}

/// Returns the current running managed fiber identifier when available.
///
/// # Errors
///
/// Returns an error if no active managed fiber is registered on the current carrier.
pub fn current_fiber_id() -> Result<FiberId, FiberError> {
    let active = current_active_fiber()?;
    active.fiber_id.ok_or_else(FiberError::state_conflict)
}

/// Returns the current running managed fiber's bound courier identifier when available.
///
/// # Errors
///
/// Returns an error if no active managed fiber is registered on the current carrier or the active
/// fiber has not been admitted under one courier identity yet.
pub fn current_courier_id() -> Result<CourierId, FiberError> {
    let active = current_active_fiber()?;
    active.courier_id.ok_or_else(FiberError::state_conflict)
}

/// Returns the current running managed fiber's bound context identifier when available.
///
/// # Errors
///
/// Returns an error when no active managed fiber is registered on the current carrier or the
/// active fiber has not been admitted under one context identity yet.
pub fn current_context_id() -> Result<ContextId, FiberError> {
    let active = current_active_fiber()?;
    active.context_id.ok_or_else(FiberError::state_conflict)
}

unsafe fn pinned_fiber_entry<T: FiberRunnable>(arg: *mut ()) -> FiberReturn {
    let state = unsafe { Pin::new_unchecked(&mut *arg.cast::<T>()) };
    T::run(state)
}

fn next_fiber_id() -> FiberId {
    static NEXT_FIBER_ID: AtomicUsize = AtomicUsize::new(1);
    FiberId::new(NEXT_FIBER_ID.fetch_add(1, Ordering::Relaxed))
}

unsafe fn fiber_entry_trampoline(context: *mut ()) -> ! {
    let bootstrap = unsafe { &mut *context.cast::<FiberBootstrap>() };
    let Some(entry) = bootstrap.entry else {
        loop {
            core::hint::spin_loop();
        }
    };
    let result = unsafe { entry(bootstrap.arg) };

    if !bootstrap.outcome.is_null() {
        unsafe {
            *bootstrap.outcome = FiberResumeOutcome::Completed(result);
        }
    }

    if !bootstrap.caller_context.is_null() && !bootstrap.fiber_context.is_null() {
        let context = system_context();
        let _ = unsafe { context.swap(&mut *bootstrap.fiber_context, &*bootstrap.caller_context) };
    }

    loop {
        core::hint::spin_loop();
    }
}

fn bootstrap_registry() -> Result<&'static FiberBootstrapRegistry, FiberError> {
    static REGISTRY: OnceLock<FiberBootstrapRegistry> = OnceLock::new();
    REGISTRY
        .get_or_init(FiberBootstrapRegistry::new)
        .map_err(fiber_error_from_sync)
}

fn with_bootstrap_slots<R>(
    f: impl FnOnce(&mut [FiberBootstrapSlot; MAX_FIBER_BOOTSTRAPS]) -> Result<R, FiberError>,
) -> Result<R, FiberError> {
    let registry = bootstrap_registry()?;
    let _guard = registry.lock.lock().map_err(fiber_error_from_sync)?;
    let slots = unsafe { &mut *registry.slots.get() };
    f(slots)
}

fn allocate_bootstrap(entry: FiberEntry, arg: *mut ()) -> Result<usize, FiberError> {
    FUSION_FIBER_BOOTSTRAP_PHASE.store(1, Ordering::Release);
    let slot = with_bootstrap_slots(|slots| {
        let (slot_index, slot) = slots
            .iter_mut()
            .enumerate()
            .find(|(_, slot)| !slot.allocated)
            .ok_or_else(FiberError::resource_exhausted)?;
        slot.allocated = true;
        slot.bootstrap = FiberBootstrap {
            entry: Some(entry),
            arg,
            caller_context: core::ptr::null_mut(),
            fiber_context: core::ptr::null_mut(),
            outcome: core::ptr::null_mut(),
        };
        Ok(slot_index)
    })?;
    FUSION_FIBER_BOOTSTRAP_PHASE.store(2, Ordering::Release);
    Ok(slot)
}

fn with_bootstrap<R>(
    slot_index: usize,
    f: impl FnOnce(&mut FiberBootstrap) -> Result<R, FiberError>,
) -> Result<R, FiberError> {
    with_bootstrap_slots(|slots| {
        let slot = slots.get_mut(slot_index).ok_or_else(FiberError::invalid)?;
        if !slot.allocated {
            return Err(FiberError::state_conflict());
        }
        f(&mut slot.bootstrap)
    })
}

fn release_bootstrap(slot_index: usize) -> Result<(), FiberError> {
    with_bootstrap_slots(|slots| {
        let slot = slots.get_mut(slot_index).ok_or_else(FiberError::invalid)?;
        if !slot.allocated {
            return Err(FiberError::state_conflict());
        }
        *slot = FiberBootstrapSlot::empty();
        Ok(())
    })
}

fn active_registry() -> Result<&'static ActiveFiberRegistry, FiberError> {
    static REGISTRY: OnceLock<ActiveFiberRegistry> = OnceLock::new();
    REGISTRY
        .get_or_init(ActiveFiberRegistry::new)
        .map_err(fiber_error_from_sync)
}

/// Primes the process-wide fiber substrate registries on the current carrier.
///
/// This is intentionally tiny and boring: some runtimes want the global bootstrap/active-fiber
/// tables realized on a known bootstrap lane before secondary carriers start materializing their
/// first fibers.
///
/// # Errors
///
/// Returns an honest error if the selected synchronization or registry substrate cannot be
/// realized.
pub fn prime_fiber_runtime_substrate() -> Result<(), FiberError> {
    let _ = bootstrap_registry()?;
    let _ = active_registry()?;
    Ok(())
}

fn with_active_slots<R>(
    f: impl FnOnce(&mut [ActiveFiberSlot; MAX_ACTIVE_FIBERS]) -> Result<R, FiberError>,
) -> Result<R, FiberError> {
    let registry = active_registry()?;
    let _guard = registry.lock.lock().map_err(fiber_error_from_sync)?;
    let slots = unsafe { &mut *registry.slots.get() };
    f(slots)
}

fn install_active_fiber(slot: ActiveFiberSlot) -> Result<(), FiberError> {
    with_active_slots(|slots| {
        if slots
            .iter()
            .any(|existing| existing.active && existing.thread_id == slot.thread_id)
        {
            return Err(FiberError::state_conflict());
        }

        let empty = slots
            .iter_mut()
            .find(|existing| !existing.active)
            .ok_or_else(FiberError::resource_exhausted)?;
        *empty = slot;
        Ok(())
    })
}

fn clear_active_fiber() -> Result<(), FiberError> {
    let thread_id = current_thread_id()?;
    with_active_slots(|slots| {
        let slot = slots
            .iter_mut()
            .find(|slot| slot.active && slot.thread_id == thread_id)
            .ok_or_else(FiberError::state_conflict)?;
        *slot = ActiveFiberSlot::empty();
        Ok(())
    })
}

fn current_active_fiber() -> Result<ActiveFiberSlot, FiberError> {
    let thread_id = current_thread_id()?;
    with_active_slots(|slots| {
        slots
            .iter()
            .copied()
            .find(|slot| slot.active && slot.thread_id == thread_id)
            .ok_or_else(FiberError::state_conflict)
    })
}

fn current_thread_id() -> Result<ThreadId, FiberError> {
    ThreadSystem::new()
        .current_thread_id()
        .map_err(fiber_error_from_thread)
}

const fn fiber_error_from_sync(error: SyncError) -> FiberError {
    match error.kind {
        SyncErrorKind::Unsupported => FiberError::unsupported(),
        SyncErrorKind::Invalid | SyncErrorKind::Overflow => FiberError::invalid(),
        SyncErrorKind::Busy | SyncErrorKind::PermissionDenied | SyncErrorKind::Platform(_) => {
            FiberError::state_conflict()
        }
    }
}

const fn fiber_error_from_thread(error: crate::thread::ThreadError) -> FiberError {
    match error.kind() {
        ThreadErrorKind::Unsupported => FiberError::unsupported(),
        ThreadErrorKind::ResourceExhausted => FiberError::resource_exhausted(),
        ThreadErrorKind::Invalid
        | ThreadErrorKind::PermissionDenied
        | ThreadErrorKind::PlacementDenied
        | ThreadErrorKind::SchedulerDenied
        | ThreadErrorKind::StackDenied
        | ThreadErrorKind::Platform(_) => FiberError::invalid(),
        ThreadErrorKind::Busy | ThreadErrorKind::Timeout | ThreadErrorKind::StateConflict => {
            FiberError::state_conflict()
        }
    }
}

const fn fiber_error_from_transport(error: TransportError) -> FiberError {
    match error.kind() {
        TransportErrorKind::Unsupported => FiberError::unsupported(),
        TransportErrorKind::Invalid => FiberError::invalid(),
        TransportErrorKind::ResourceExhausted => FiberError::resource_exhausted(),
        TransportErrorKind::Busy
        | TransportErrorKind::PermissionDenied
        | TransportErrorKind::StateConflict
        | TransportErrorKind::NotAttached
        | TransportErrorKind::Platform(_) => FiberError::state_conflict(),
    }
}

const fn fiber_error_from_channel(error: ChannelError) -> FiberError {
    match error.kind() {
        crate::channel::ChannelErrorKind::Unsupported => FiberError::unsupported(),
        crate::channel::ChannelErrorKind::Invalid
        | crate::channel::ChannelErrorKind::ProtocolMismatch => FiberError::invalid(),
        crate::channel::ChannelErrorKind::ResourceExhausted => FiberError::resource_exhausted(),
        crate::channel::ChannelErrorKind::Busy
        | crate::channel::ChannelErrorKind::PermissionDenied
        | crate::channel::ChannelErrorKind::StateConflict
        | crate::channel::ChannelErrorKind::TransportDenied
        | crate::channel::ChannelErrorKind::Platform(_) => FiberError::state_conflict(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::{
        AtomicUsize,
        Ordering,
    };
    use std::sync::{
        Mutex,
        OnceLock,
    };

    fn fiber_test_guard() -> std::sync::MutexGuard<'static, ()> {
        static FIBER_TEST_GUARD: OnceLock<Mutex<()>> = OnceLock::new();
        FIBER_TEST_GUARD
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn reset_active_slots() {
        with_active_slots(|slots| {
            *slots = [ActiveFiberSlot::empty(); MAX_ACTIVE_FIBERS];
            Ok(())
        })
        .expect("active slot reset should succeed");
    }

    #[test]
    fn install_active_fiber_rejects_duplicate_thread_slot() {
        let _guard = fiber_test_guard();
        reset_active_slots();

        let thread_id = current_thread_id().expect("thread id should be available");
        let slot = ActiveFiberSlot {
            active: true,
            thread_id,
            fiber_id: None,
            courier_id: None,
            context_id: None,
            arg: core::ptr::null_mut(),
            caller_context: core::ptr::null_mut(),
            fiber_context: core::ptr::null_mut(),
            outcome: core::ptr::null_mut(),
        };

        install_active_fiber(slot).expect("first active install should succeed");
        assert!(matches!(
            install_active_fiber(slot),
            Err(error) if error.kind() == FiberErrorKind::StateConflict
        ));

        clear_active_fiber().expect("active slot should clear");
    }

    struct YieldOnce;

    impl FiberRunnable for YieldOnce {
        fn run(self: Pin<&mut Self>) -> FiberReturn {
            let _ = yield_now();
            FiberReturn::new(0)
        }
    }

    struct CaptureCurrentFiberId<'a> {
        observed: &'a AtomicUsize,
    }

    impl FiberRunnable for CaptureCurrentFiberId<'_> {
        fn run(self: Pin<&mut Self>) -> FiberReturn {
            let state = self.get_mut();
            let id = current_fiber_id().expect("managed fiber id should be visible while running");
            state.observed.store(id.get(), Ordering::Release);
            let _ = yield_now();
            FiberReturn::new(0)
        }
    }

    struct CaptureCurrentCourierId<'a> {
        observed: &'a AtomicUsize,
    }

    impl FiberRunnable for CaptureCurrentCourierId<'_> {
        fn run(self: Pin<&mut Self>) -> FiberReturn {
            let state = self.get_mut();
            let id =
                current_courier_id().expect("managed courier id should be visible while running");
            state.observed.store(id.get() as usize, Ordering::Release);
            let _ = yield_now();
            FiberReturn::new(0)
        }
    }

    #[test]
    fn managed_fiber_claim_mode_requires_context_for_black() {
        let _guard = fiber_test_guard();
        let mut stack = [0u128; 512];
        let stack = FiberStack::from_slice(&mut stack).expect("stack should build");
        let mut state = YieldOnce;
        let mut managed = ManagedFiber::<_, 8>::new(Pin::new(&mut state), stack)
            .expect("managed fiber should build");
        assert!(managed.metadata_channel().is_none());
        let initial = managed.snapshot();
        assert_eq!(initial.state, FiberState::Created);
        assert!(!initial.started);
        assert!(!initial.is_claim_enabled());

        assert!(matches!(
            managed.set_claim_mode(ClaimAwareness::Black, None),
            Err(error) if error.kind() == FiberErrorKind::Invalid
        ));
        managed
            .set_claim_mode(ClaimAwareness::Black, Some(ClaimContextId::new(9)))
            .expect("black claim mode should accept one context");
        assert_eq!(managed.claim_awareness(), ClaimAwareness::Black);
        assert_eq!(managed.claim_context(), Some(ClaimContextId::new(9)));
        let after_claims = managed.snapshot();
        assert_eq!(after_claims.claim_context, Some(ClaimContextId::new(9)));
        assert!(after_claims.is_claim_enabled());
    }

    #[test]
    fn managed_fiber_resume_exposes_current_fiber_id() {
        let _guard = fiber_test_guard();
        let mut stack = [0u128; 512];
        let stack = FiberStack::from_slice(&mut stack).expect("stack should build");
        let observed = AtomicUsize::new(0);
        let mut state = CaptureCurrentFiberId {
            observed: &observed,
        };
        let mut managed = ManagedFiber::<_, 8>::new(Pin::new(&mut state), stack)
            .expect("managed fiber should build");

        assert_eq!(
            managed.resume().expect("managed fiber should yield"),
            FiberYield::Yielded
        );
        assert_eq!(observed.load(Ordering::Acquire), managed.id().get());
    }

    #[test]
    fn managed_fiber_resume_exposes_current_courier_id_when_bound() {
        let _guard = fiber_test_guard();
        let mut stack = [0u128; 512];
        let stack = FiberStack::from_slice(&mut stack).expect("stack should build");
        let observed = AtomicUsize::new(0);
        let mut state = CaptureCurrentCourierId {
            observed: &observed,
        };
        let mut managed = ManagedFiber::<_, 8>::new(Pin::new(&mut state), stack)
            .expect("managed fiber should build");
        managed.bind_to_courier(CourierId::new(33));

        assert_eq!(
            managed.resume().expect("managed fiber should yield"),
            FiberYield::Yielded
        );
        assert_eq!(observed.load(Ordering::Acquire), 33);
    }
}
