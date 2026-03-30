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
use core::sync::atomic::{AtomicUsize, Ordering};

pub use fusion_pal::sys::execution_context::{
    ContextAuthoritySet,
    ContextBase,
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
};

use crate::channel::{ChannelError, ChannelSend, LocalChannel};
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
use crate::sync::{OnceLock, SyncError, SyncErrorKind, ThinMutex};
use crate::thread::{ThreadErrorKind, ThreadId, ThreadSystem};
use crate::transport::{
    TransportAttachmentControl,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FiberMetadataMessage {
    /// One managed fiber was created and now owns one metadata channel.
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
    /// One managed fiber was dropped before terminal completion.
    Abandoned {
        fiber: FiberId,
        lifecycle: FiberState,
    },
}

/// Managed-fiber metadata protocol carried over the automatic fiber metadata channel.
pub struct FiberMetadataProtocol;

impl Protocol for FiberMetadataProtocol {
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

/// Automatic metadata channel carried by one managed fiber.
pub type FiberMetadataChannel<const CAPACITY: usize, const MAX_CONSUMERS: usize = 8> =
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

/// One subsystem-facing managed fiber with an automatic metadata channel.
pub struct ManagedFiber<
    'state,
    T: FiberRunnable,
    const META_CAPACITY: usize = 16,
    const MAX_CONSUMERS: usize = 8,
> {
    id: FiberId,
    started: bool,
    fiber: Option<PinnedFiber<'state, T>>,
    metadata: FiberMetadataChannel<META_CAPACITY, MAX_CONSUMERS>,
    metadata_producer: usize,
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
            context: ContextBase::support(&self.inner),
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
    /// Creates one managed fiber with an automatic metadata channel.
    ///
    /// # Errors
    ///
    /// Returns any honest fiber or metadata-channel construction failure.
    pub fn new(state: Pin<&'state mut T>, stack: FiberStack) -> Result<Self, FiberError> {
        let fiber = PinnedFiber::new(state, stack)?;
        let metadata = FiberMetadataChannel::<META_CAPACITY, MAX_CONSUMERS>::new()
            .map_err(fiber_error_from_channel)?;
        let metadata_producer = metadata
            .attach_producer(TransportAttachmentRequest::same_courier())
            .map_err(fiber_error_from_transport)?;
        let managed = Self {
            id: next_fiber_id(),
            started: false,
            fiber: Some(fiber),
            metadata,
            metadata_producer,
        };
        managed.publish_metadata(FiberMetadataMessage::Created { fiber: managed.id });
        Ok(managed)
    }

    /// Returns the stable managed-fiber identifier.
    #[must_use]
    pub const fn id(&self) -> FiberId {
        self.id
    }

    /// Returns the automatic metadata channel owned by this fiber.
    #[must_use]
    pub const fn metadata_channel(&self) -> &FiberMetadataChannel<META_CAPACITY, MAX_CONSUMERS> {
        &self.metadata
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
            .resume()
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
        let _ = self.metadata.try_send(self.metadata_producer, message);
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
        let context = system_context();
        let support = context.support();
        if !support.caps.contains(ContextCaps::MAKE) || !support.caps.contains(ContextCaps::SWAP) {
            return Err(FiberError::unsupported());
        }

        let mut fiber = Self {
            context: PlatformSavedContext::default(),
            bootstrap_slot: None,
            outcome: FiberResumeOutcome::None,
            stack,
            state: FiberState::Created,
        };

        let bootstrap_slot = allocate_bootstrap(entry, arg)?;
        let stack_layout = ContextStackLayout {
            base: stack.base,
            len: stack.len,
        };
        let bootstrap_ptr = with_bootstrap(bootstrap_slot, |bootstrap| {
            Ok(core::ptr::from_mut(bootstrap).cast())
        })?;
        fiber.context =
            match unsafe { context.make(stack_layout, fiber_entry_trampoline, bootstrap_ptr) } {
                Ok(saved) => saved,
                Err(error) => {
                    release_bootstrap(bootstrap_slot)?;
                    return Err(FiberError::from(error));
                }
            };
        fiber.bootstrap_slot = Some(bootstrap_slot);
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

    /// Creates one managed typed stackful fiber with an automatic metadata channel.
    ///
    /// # Errors
    ///
    /// Returns any honest low-level fiber or metadata-channel construction failure.
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

    /// Resumes the fiber on the current carrier thread.
    ///
    /// # Errors
    ///
    /// Returns an error if the fiber is already running or has completed, or the selected
    /// backend cannot resume the saved context honestly.
    pub fn resume(&mut self) -> Result<FiberYield, FiberError> {
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
    with_bootstrap_slots(|slots| {
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
    })
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

    fn reset_active_slots() {
        with_active_slots(|slots| {
            *slots = [ActiveFiberSlot::empty(); MAX_ACTIVE_FIBERS];
            Ok(())
        })
        .expect("active slot reset should succeed");
    }

    #[test]
    fn install_active_fiber_rejects_duplicate_thread_slot() {
        reset_active_slots();

        let thread_id = current_thread_id().expect("thread id should be available");
        let slot = ActiveFiberSlot {
            active: true,
            thread_id,
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
}
