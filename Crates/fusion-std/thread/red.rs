//! Urgent red-execution class.

use crate::thread::fiber::{CooperativeExclusionSpan, CooperativeExclusionSummaryTree};
#[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
use crate::thread::fiber::{current_green_exclusion_allows, current_green_exclusion_allows_tree};
use fusion_sys::thread::{
    ThreadConfig,
    ThreadError,
    ThreadGuarantee,
    ThreadPlacementOutcome,
    ThreadSchedulerObservation,
    ThreadSupport,
    ThreadSystem,
};
use fusion_sys::vector::{
    VectorDispatchCookie,
    VectorDispatchLane,
    VectorInlineHandler,
    VectorInlineStackPolicy,
    VectorTableBuilder,
};

#[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
use fusion_pal::sys::hal::soc::board as cortex_m_board;
#[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
use fusion_sys::vector::VectorInlineEligibility;

/// Dispatch policy for one red-thread request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RedDispatchPolicy {
    /// Spawn immediately or fail honestly.
    ImmediateOrReject,
    /// Queue for later dispatch when no urgent execution capacity is available.
    QueueIfBusy,
}

/// Reservation policy for one red-thread request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RedReservationPolicy {
    /// Use ordinary native system-thread capacity.
    SharedSystem,
    /// Require one dedicated worker lane.
    DedicatedWorker,
    /// Prefer, but do not require, one dedicated performance core.
    DedicatedCorePreferred,
    /// Require one dedicated performance core.
    DedicatedCoreRequired,
}

/// Prompt or wake policy for one red-thread request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RedPromptPolicy {
    /// Do not request extra prompt or wake semantics.
    None,
    /// Request an event-style wake path when the backend can support it honestly.
    WakeEvent,
    /// Request an interrupt-prompted wake path when the backend can support it honestly.
    InterruptPrompted,
}

/// Power policy for one red-thread request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RedPowerPolicy<'a> {
    /// Inherit backend or process-default power behavior.
    Inherit,
    /// Request a named power mode or wake policy before urgent dispatch.
    EnterMode(&'a str),
}

/// Public configuration for one red-thread spawn request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RedThreadConfig<'a> {
    /// Dispatch policy.
    pub dispatch: RedDispatchPolicy,
    /// Reservation policy.
    pub reservation: RedReservationPolicy,
    /// Prompt policy.
    pub prompt: RedPromptPolicy,
    /// Power policy.
    pub power: RedPowerPolicy<'a>,
    /// Underlying native thread configuration.
    pub thread: ThreadConfig<'a>,
}

impl<'a> RedThreadConfig<'a> {
    /// Returns a minimal urgent-thread configuration over the ordinary thread substrate.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            dispatch: RedDispatchPolicy::ImmediateOrReject,
            reservation: RedReservationPolicy::SharedSystem,
            prompt: RedPromptPolicy::None,
            power: RedPowerPolicy::Inherit,
            thread: ThreadConfig::new(),
        }
    }

    /// Returns one copy with an explicit dispatch policy.
    #[must_use]
    pub const fn with_dispatch(mut self, dispatch: RedDispatchPolicy) -> Self {
        self.dispatch = dispatch;
        self
    }

    /// Returns one copy with an explicit reservation policy.
    #[must_use]
    pub const fn with_reservation(mut self, reservation: RedReservationPolicy) -> Self {
        self.reservation = reservation;
        self
    }

    /// Returns one copy with an explicit prompt policy.
    #[must_use]
    pub const fn with_prompt(mut self, prompt: RedPromptPolicy) -> Self {
        self.prompt = prompt;
        self
    }

    /// Returns one copy with an explicit power policy.
    #[must_use]
    pub const fn with_power(mut self, power: RedPowerPolicy<'a>) -> Self {
        self.power = power;
        self
    }

    /// Returns one copy with an explicit native thread configuration.
    #[must_use]
    pub const fn with_thread(mut self, thread: ThreadConfig<'a>) -> Self {
        self.thread = thread;
        self
    }
}

impl Default for RedThreadConfig<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// Capability grading for the red-thread execution class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RedThreadSupport {
    /// Underlying native thread support surface.
    pub thread: ThreadSupport,
    /// Strength of immediate dispatch.
    pub immediate_dispatch: ThreadGuarantee,
    /// Strength of queued urgent dispatch.
    pub queued_dispatch: ThreadGuarantee,
    /// Strength of shared-system reservation.
    pub shared_system_reservation: ThreadGuarantee,
    /// Strength of dedicated-worker reservation.
    pub dedicated_worker_reservation: ThreadGuarantee,
    /// Strength of dedicated-core reservation.
    pub dedicated_core_reservation: ThreadGuarantee,
    /// Strength of event-style wake prompting.
    pub wake_event_prompt: ThreadGuarantee,
    /// Strength of interrupt-style wake prompting.
    pub interrupt_prompt: ThreadGuarantee,
    /// Strength of binding urgent execution to one hardware IRQ lane.
    pub interrupt_binding: ThreadGuarantee,
    /// Strength of raw hardware-IRQ priority control.
    pub interrupt_priority_control: ThreadGuarantee,
    /// Strength of explicit power-mode control in the urgent path.
    pub power_control: ThreadGuarantee,
}

/// Effective admission snapshot for one red-thread spawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RedThreadAdmission {
    /// Strength of the effective reservation behavior.
    pub reservation: ThreadGuarantee,
    /// Strength of the effective prompt behavior.
    pub prompt: ThreadGuarantee,
    /// Effective placement observation after spawn.
    pub placement: ThreadPlacementOutcome,
    /// Effective scheduler observation after spawn.
    pub scheduler: ThreadSchedulerObservation,
}

/// Public configuration for one interrupt-bound red execution lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RedInterruptConfig<'a> {
    /// Bound external IRQ line.
    pub irqn: u16,
    /// Raw board-defined IRQ priority byte, when one should be applied.
    pub priority: Option<u8>,
    /// Whether the IRQ line should be enabled immediately after bind.
    pub enable_on_bind: bool,
    /// Whether the NVIC pending bit should be cleared during bind.
    pub clear_pending_on_bind: bool,
    /// Stack policy used when the owned-table inline trampoline enters the handler body.
    pub stack: VectorInlineStackPolicy,
    /// Prompt policy for later urgent dispatch.
    pub prompt: RedPromptPolicy,
    /// Power policy requested for the urgent path.
    pub power: RedPowerPolicy<'a>,
    /// Optional inline-admission compatibility contract for owned-table bindings.
    pub inline_compatibility: Option<&'static RedInlineCompatibility>,
}

impl<'a> RedInterruptConfig<'a> {
    /// Returns a minimal interrupt-bound red execution configuration.
    ///
    /// The line is intentionally left disabled by default because Fusion does not own the vector
    /// table. Callers must install a valid IRQ handler before enabling or pending the line.
    #[must_use]
    pub const fn new(irqn: u16) -> Self {
        Self {
            irqn,
            priority: None,
            enable_on_bind: false,
            clear_pending_on_bind: true,
            stack: VectorInlineStackPolicy::CurrentExceptionStack,
            prompt: RedPromptPolicy::None,
            power: RedPowerPolicy::Inherit,
            inline_compatibility: None,
        }
    }

    /// Returns one copy with an explicit raw IRQ priority byte.
    #[must_use]
    pub const fn with_priority(mut self, priority: u8) -> Self {
        self.priority = Some(priority);
        self
    }

    /// Returns one copy with an explicit enable-on-bind policy.
    #[must_use]
    pub const fn with_enable_on_bind(mut self, enable_on_bind: bool) -> Self {
        self.enable_on_bind = enable_on_bind;
        self
    }

    /// Returns one copy with an explicit pending-clear policy for bind.
    #[must_use]
    pub const fn with_clear_pending_on_bind(mut self, clear_pending_on_bind: bool) -> Self {
        self.clear_pending_on_bind = clear_pending_on_bind;
        self
    }

    /// Returns one copy with an explicit inline stack policy.
    #[must_use]
    pub const fn with_stack(mut self, stack: VectorInlineStackPolicy) -> Self {
        self.stack = stack;
        self
    }

    /// Returns one copy with an explicit prompt policy.
    #[must_use]
    pub const fn with_prompt(mut self, prompt: RedPromptPolicy) -> Self {
        self.prompt = prompt;
        self
    }

    /// Returns one copy with an explicit power policy.
    #[must_use]
    pub const fn with_power(mut self, power: RedPowerPolicy<'a>) -> Self {
        self.power = power;
        self
    }

    /// Returns one copy with one explicit inline compatibility contract.
    #[must_use]
    pub const fn with_inline_compatibility(
        mut self,
        inline_compatibility: &'static RedInlineCompatibility,
    ) -> Self {
        self.inline_compatibility = Some(inline_compatibility);
        self
    }
}

impl Default for RedInterruptConfig<'_> {
    fn default() -> Self {
        Self::new(u16::MAX)
    }
}

/// One owned inline red-admission contract backed by active green exclusion spans.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RedInlineClearRequirement {
    /// Explicit named exclusion spans that must all be clear.
    Spans(&'static [CooperativeExclusionSpan]),
    /// One compile-time summary tree over named exclusion spans that must all be clear.
    SummaryTree(&'static CooperativeExclusionSummaryTree),
}

/// One owned inline red-admission contract backed by active green exclusion state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RedInlineCompatibility {
    /// Exclusion requirement that must be clear before inline urgent execution is allowed.
    pub required_clear: RedInlineClearRequirement,
    /// Minimum current-exception stack headroom required before inline execution may run on the
    /// live exception stack. `0` means “no additional current-stack requirement”.
    pub required_current_exception_stack_bytes: usize,
    /// Deferred vector lane used when inline urgent execution is currently incompatible.
    pub fallback_lane: VectorDispatchLane,
    /// Opaque deferred cookie surfaced when inline urgent execution falls back.
    pub fallback_cookie: VectorDispatchCookie,
}

impl RedInlineCompatibility {
    /// Creates one compatibility contract from explicit named exclusion spans.
    #[must_use]
    pub const fn from_spans(
        required_clear_spans: &'static [CooperativeExclusionSpan],
        fallback_lane: VectorDispatchLane,
        fallback_cookie: VectorDispatchCookie,
    ) -> Self {
        Self {
            required_clear: RedInlineClearRequirement::Spans(required_clear_spans),
            required_current_exception_stack_bytes: 0,
            fallback_lane,
            fallback_cookie,
        }
    }

    /// Creates one compatibility contract from one compile-time summary tree.
    #[must_use]
    pub const fn from_summary_tree(
        required_clear_tree: &'static CooperativeExclusionSummaryTree,
        fallback_lane: VectorDispatchLane,
        fallback_cookie: VectorDispatchCookie,
    ) -> Self {
        Self {
            required_clear: RedInlineClearRequirement::SummaryTree(required_clear_tree),
            required_current_exception_stack_bytes: 0,
            fallback_lane,
            fallback_cookie,
        }
    }

    /// Returns one copy with an explicit current-exception stack headroom requirement.
    #[must_use]
    pub const fn with_current_exception_stack_bytes(
        mut self,
        required_current_exception_stack_bytes: usize,
    ) -> Self {
        self.required_current_exception_stack_bytes = required_current_exception_stack_bytes;
        self
    }
}

/// Includes one generated Rust sidecar emitted by the analyzer pipeline for red inline
/// compatibility contracts.
#[macro_export]
macro_rules! include_generated_red_inline_contracts {
    ($path:expr $(,)?) => {
        include!($path);
    };
}

/// Effective admission snapshot for one interrupt-bound red execution lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RedInterruptAdmission {
    /// Strength of the effective interrupt-priority control.
    pub priority: ThreadGuarantee,
    /// Strength of the effective prompt behavior.
    pub prompt: ThreadGuarantee,
    /// Strength of the effective power behavior.
    pub power: ThreadGuarantee,
}

/// Error returned when joining a red thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RedThreadJoinError {
    /// Native thread join or observation failed.
    Thread(ThreadError),
    /// The thread entry panicked before producing a value.
    Panicked,
}

impl From<ThreadError> for RedThreadJoinError {
    fn from(value: ThreadError) -> Self {
        Self::Thread(value)
    }
}

/// Native urgent-thread execution handle.
#[cfg(feature = "std")]
#[derive(Debug)]
pub struct RedThread<T> {
    handle: Option<fusion_sys::thread::ThreadHandle>,
    result: std::sync::Arc<std::sync::Mutex<Option<Result<T, RedThreadJoinError>>>>,
    admission: RedThreadAdmission,
}

/// Interrupt-bound urgent execution handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RedInterrupt {
    irqn: u16,
    admission: RedInterruptAdmission,
}

impl RedThreadSupport {
    #[must_use]
    const fn unsupported(thread: ThreadSupport) -> Self {
        Self {
            thread,
            immediate_dispatch: ThreadGuarantee::Unsupported,
            queued_dispatch: ThreadGuarantee::Unsupported,
            shared_system_reservation: ThreadGuarantee::Unsupported,
            dedicated_worker_reservation: ThreadGuarantee::Unsupported,
            dedicated_core_reservation: ThreadGuarantee::Unsupported,
            wake_event_prompt: ThreadGuarantee::Unsupported,
            interrupt_prompt: ThreadGuarantee::Unsupported,
            interrupt_binding: ThreadGuarantee::Unsupported,
            interrupt_priority_control: ThreadGuarantee::Unsupported,
            power_control: ThreadGuarantee::Unsupported,
        }
    }
}

/// Returns the current red-thread support surface.
#[must_use]
pub fn red_thread_support() -> RedThreadSupport {
    let thread = ThreadSystem::new().support();
    let mut support = RedThreadSupport::unsupported(thread);

    if thread
        .lifecycle
        .caps
        .contains(fusion_sys::thread::ThreadLifecycleCaps::SPAWN)
        && thread
            .lifecycle
            .caps
            .contains(fusion_sys::thread::ThreadLifecycleCaps::JOIN)
    {
        support.immediate_dispatch = ThreadGuarantee::Verified;
        support.shared_system_reservation = ThreadGuarantee::Verified;
        support.dedicated_core_reservation =
            if thread.placement.core_class_affinity == ThreadGuarantee::Unsupported {
                ThreadGuarantee::Unsupported
            } else {
                ThreadGuarantee::Advisory
            };
    }

    #[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
    {
        if !cortex_m_board::irqs().is_empty() {
            support.interrupt_binding = ThreadGuarantee::Verified;
            support.interrupt_prompt = ThreadGuarantee::Verified;
        }
        if cortex_m_board::irqs()
            .iter()
            .any(|descriptor| cortex_m_board::irq_priority_supported(descriptor.irqn))
        {
            support.interrupt_priority_control = ThreadGuarantee::Verified;
        }
    }

    support
}

#[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
const fn map_cortex_m_hardware_error(error: fusion_pal::pal::hal::HardwareError) -> ThreadError {
    match error.kind() {
        fusion_pal::pal::hal::HardwareErrorKind::Unsupported => ThreadError::unsupported(),
        fusion_pal::pal::hal::HardwareErrorKind::Invalid => ThreadError::invalid(),
        fusion_pal::pal::hal::HardwareErrorKind::ResourceExhausted => {
            ThreadError::resource_exhausted()
        }
        fusion_pal::pal::hal::HardwareErrorKind::StateConflict => ThreadError::state_conflict(),
        fusion_pal::pal::hal::HardwareErrorKind::Busy => ThreadError::busy(),
        fusion_pal::pal::hal::HardwareErrorKind::Platform(code) => ThreadError::platform(code),
    }
}

#[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
fn cortex_m_irq_exists(irqn: u16) -> bool {
    cortex_m_board::irqs()
        .iter()
        .any(|descriptor| descriptor.irqn == irqn)
}

#[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
fn validate_red_interrupt_config(
    config: &RedInterruptConfig<'_>,
) -> Result<RedInterruptAdmission, ThreadError> {
    let support = red_thread_support();
    if support.interrupt_binding == ThreadGuarantee::Unsupported {
        return Err(ThreadError::unsupported());
    }
    if !cortex_m_irq_exists(config.irqn) {
        return Err(ThreadError::invalid());
    }
    if config.priority.is_some() && !cortex_m_board::irq_priority_supported(config.irqn) {
        return Err(ThreadError::unsupported());
    }
    if matches!(config.prompt, RedPromptPolicy::WakeEvent) {
        return Err(ThreadError::unsupported());
    }
    if matches!(config.power, RedPowerPolicy::EnterMode(_)) {
        return Err(ThreadError::unsupported());
    }
    if config
        .inline_compatibility
        .is_some_and(|contract| matches!(contract.fallback_lane, VectorDispatchLane::Inline))
    {
        return Err(ThreadError::invalid());
    }

    Ok(RedInterruptAdmission {
        priority: if config.priority.is_some() {
            ThreadGuarantee::Verified
        } else {
            ThreadGuarantee::Unsupported
        },
        prompt: match config.prompt {
            RedPromptPolicy::None => ThreadGuarantee::Verified,
            RedPromptPolicy::InterruptPrompted => ThreadGuarantee::Verified,
            RedPromptPolicy::WakeEvent => ThreadGuarantee::Unsupported,
        },
        power: match config.power {
            RedPowerPolicy::Inherit => ThreadGuarantee::Verified,
            RedPowerPolicy::EnterMode(_) => ThreadGuarantee::Unsupported,
        },
    })
}

#[allow(clippy::missing_const_for_fn)]
impl RedInterrupt {
    /// Reports the current red interrupt-lane support surface.
    #[must_use]
    pub fn support() -> RedThreadSupport {
        red_thread_support()
    }

    /// Binds one interrupt-backed urgent execution lane.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `config.irqn` is backed by a valid interrupt handler before
    /// requesting `enable_on_bind` or later calling [`Self::pend`]. Fusion does not own the
    /// vector table, so it cannot prove that handing control to this IRQ line will land anywhere
    /// sane.
    ///
    /// # Errors
    ///
    /// Returns any honest unsupported policy or lower-level IRQ-control failure.
    pub unsafe fn bind(config: &RedInterruptConfig<'_>) -> Result<Self, ThreadError> {
        #[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
        {
            let admission = validate_red_interrupt_config(config)?;
            if config.clear_pending_on_bind {
                cortex_m_board::irq_clear_pending(config.irqn)
                    .map_err(map_cortex_m_hardware_error)?;
            }
            if let Some(priority) = config.priority {
                cortex_m_board::irq_set_priority(config.irqn, priority)
                    .map_err(map_cortex_m_hardware_error)?;
            }
            if config.enable_on_bind {
                cortex_m_board::irq_enable(config.irqn).map_err(map_cortex_m_hardware_error)?;
            }
            return Ok(Self {
                irqn: config.irqn,
                admission,
            });
        }

        #[cfg(not(all(target_os = "none", feature = "sys-cortex-m")))]
        {
            let _ = config;
            Err(ThreadError::unsupported())
        }
    }

    /// Safely binds one interrupt-backed urgent execution lane through an owned vector-table
    /// builder.
    ///
    /// This is the owned-dispatch path: the caller presents one mutable vector-table builder that
    /// proves Fusion controls the slot entry, so the runtime no longer has to pretend a random
    /// existing vector-table entry probably points somewhere civilized.
    ///
    /// # Errors
    ///
    /// Returns any honest unsupported policy, vector-binding failure, or lower-level IRQ-control
    /// failure.
    pub fn bind_owned(
        builder: &mut VectorTableBuilder,
        config: &RedInterruptConfig<'_>,
        handler: VectorInlineHandler,
    ) -> Result<Self, ThreadError> {
        #[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
        {
            let admission = validate_red_interrupt_config(config)?;
            if let Some(contract) = config.inline_compatibility
                && !builder
                    .deferred_callback_registered(contract.fallback_cookie)
                    .map_err(map_vector_error)?
            {
                return Err(ThreadError::state_conflict());
            }
            let eligibility = config
                .inline_compatibility
                .map(|contract| VectorInlineEligibility {
                    context: core::ptr::from_ref(contract).cast(),
                    allow_now: red_inline_compatibility_allows_now,
                    required_current_exception_stack_bytes: contract
                        .required_current_exception_stack_bytes,
                    fallback_lane: contract.fallback_lane,
                    fallback_cookie: contract.fallback_cookie,
                });
            builder
                .bind_inline_with_eligibility(
                    fusion_sys::vector::IrqSlot(config.irqn),
                    None,
                    config.priority.map(fusion_sys::vector::VectorPriority),
                    handler,
                    config.stack,
                    eligibility,
                )
                .map_err(map_vector_error)?;

            let interrupt = Self {
                irqn: config.irqn,
                admission,
            };
            if config.clear_pending_on_bind {
                interrupt.clear_pending()?;
            }
            if config.enable_on_bind {
                interrupt.enable()?;
            }
            return Ok(interrupt);
        }

        #[cfg(not(all(target_os = "none", feature = "sys-cortex-m")))]
        {
            let _ = (builder, config, handler);
            Err(ThreadError::unsupported())
        }
    }

    /// Returns the bound IRQ line.
    #[must_use]
    pub const fn irqn(&self) -> u16 {
        self.irqn
    }

    /// Returns the effective admission snapshot for this urgent interrupt lane.
    #[must_use]
    pub const fn admission(&self) -> RedInterruptAdmission {
        self.admission
    }

    /// Enables the bound IRQ line.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot enable the line honestly.
    pub fn enable(&self) -> Result<(), ThreadError> {
        #[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
        {
            return cortex_m_board::irq_enable(self.irqn).map_err(map_cortex_m_hardware_error);
        }

        #[cfg(not(all(target_os = "none", feature = "sys-cortex-m")))]
        {
            Err(ThreadError::unsupported())
        }
    }

    /// Disables the bound IRQ line.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot disable the line honestly.
    pub fn disable(&self) -> Result<(), ThreadError> {
        #[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
        {
            return cortex_m_board::irq_disable(self.irqn).map_err(map_cortex_m_hardware_error);
        }

        #[cfg(not(all(target_os = "none", feature = "sys-cortex-m")))]
        {
            Err(ThreadError::unsupported())
        }
    }

    /// Software-pends the bound IRQ line.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot software-pend the bound IRQ honestly.
    pub fn pend(&self) -> Result<(), ThreadError> {
        #[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
        {
            return cortex_m_board::irq_set_pending(self.irqn).map_err(map_cortex_m_hardware_error);
        }

        #[cfg(not(all(target_os = "none", feature = "sys-cortex-m")))]
        {
            Err(ThreadError::unsupported())
        }
    }

    /// Clears the NVIC pending state for the bound IRQ line.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot clear the bound pending state honestly.
    pub fn clear_pending(&self) -> Result<(), ThreadError> {
        #[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
        {
            return cortex_m_board::irq_clear_pending(self.irqn)
                .map_err(map_cortex_m_hardware_error);
        }

        #[cfg(not(all(target_os = "none", feature = "sys-cortex-m")))]
        {
            Err(ThreadError::unsupported())
        }
    }

    /// Acknowledges the bound IRQ line when the board contract owns a generic clear path.
    ///
    /// # Errors
    ///
    /// Returns an error if the line cannot be acknowledged generically.
    pub fn acknowledge(&self) -> Result<(), ThreadError> {
        #[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
        {
            return cortex_m_board::irq_acknowledge(self.irqn).map_err(map_cortex_m_hardware_error);
        }

        #[cfg(not(all(target_os = "none", feature = "sys-cortex-m")))]
        {
            Err(ThreadError::unsupported())
        }
    }

    /// Executes one urgent job inside the already-entered interrupt context.
    ///
    /// This does not install or own the actual handler. It is the narrow semantic marker for
    /// “run this urgent work on the bound IRQ lane that the hardware already transferred us to”.
    #[must_use]
    pub fn service<F, T>(&self, job: F) -> T
    where
        F: FnOnce() -> T,
    {
        let _ = self;
        job()
    }
}

#[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
unsafe extern "C" fn red_inline_compatibility_allows_now(context: *const ()) -> bool {
    let contract = unsafe { &*context.cast::<RedInlineCompatibility>() };
    match contract.required_clear {
        RedInlineClearRequirement::Spans(spans) => current_green_exclusion_allows(spans),
        RedInlineClearRequirement::SummaryTree(tree) => current_green_exclusion_allows_tree(tree),
    }
}

#[cfg(all(target_os = "none", feature = "sys-cortex-m"))]
const fn map_vector_error(error: fusion_sys::vector::VectorError) -> ThreadError {
    match error.kind() {
        fusion_sys::vector::VectorErrorKind::Unsupported => ThreadError::unsupported(),
        fusion_sys::vector::VectorErrorKind::Invalid
        | fusion_sys::vector::VectorErrorKind::Reserved
        | fusion_sys::vector::VectorErrorKind::CoreMismatch
        | fusion_sys::vector::VectorErrorKind::WorldMismatch => ThreadError::invalid(),
        fusion_sys::vector::VectorErrorKind::AlreadyBound
        | fusion_sys::vector::VectorErrorKind::NotBound
        | fusion_sys::vector::VectorErrorKind::StateConflict
        | fusion_sys::vector::VectorErrorKind::SealViolation
        | fusion_sys::vector::VectorErrorKind::Sealed => ThreadError::state_conflict(),
        fusion_sys::vector::VectorErrorKind::ResourceExhausted => ThreadError::resource_exhausted(),
        fusion_sys::vector::VectorErrorKind::Platform(code) => ThreadError::platform(code),
    }
}

#[cfg(feature = "std")]
fn validate_red_thread_config(
    config: &RedThreadConfig<'_>,
) -> Result<RedThreadAdmission, ThreadError> {
    let support = red_thread_support();
    if support.immediate_dispatch == ThreadGuarantee::Unsupported {
        return Err(ThreadError::unsupported());
    }
    if config.thread.join_policy != fusion_sys::thread::ThreadJoinPolicy::Joinable {
        return Err(ThreadError::invalid());
    }
    if matches!(config.dispatch, RedDispatchPolicy::QueueIfBusy) {
        return Err(ThreadError::unsupported());
    }
    let reservation = match config.reservation {
        RedReservationPolicy::SharedSystem => ThreadGuarantee::Verified,
        RedReservationPolicy::DedicatedWorker | RedReservationPolicy::DedicatedCoreRequired => {
            return Err(ThreadError::unsupported());
        }
        RedReservationPolicy::DedicatedCorePreferred => ThreadGuarantee::Advisory,
    };
    let prompt = match config.prompt {
        RedPromptPolicy::None => ThreadGuarantee::Verified,
        RedPromptPolicy::WakeEvent | RedPromptPolicy::InterruptPrompted => {
            return Err(ThreadError::unsupported());
        }
    };
    match config.power {
        RedPowerPolicy::Inherit => {}
        RedPowerPolicy::EnterMode(_) => return Err(ThreadError::unsupported()),
    }

    Ok(RedThreadAdmission {
        reservation,
        prompt,
        placement: ThreadPlacementOutcome::unsupported(),
        scheduler: ThreadSchedulerObservation::unknown(),
    })
}

#[cfg(feature = "std")]
struct RedThreadStart<F, T> {
    job: Option<F>,
    result: std::sync::Arc<std::sync::Mutex<Option<Result<T, RedThreadJoinError>>>>,
}

#[cfg(feature = "std")]
unsafe fn red_thread_entry<F, T>(context: *mut ()) -> fusion_sys::thread::ThreadEntryReturn
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    // SAFETY: `spawn` passes one boxed start record matching the monomorphized entry type.
    let mut start = unsafe { std::boxed::Box::from_raw(context.cast::<RedThreadStart<F, T>>()) };
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let job = start
            .job
            .take()
            .expect("red thread entry should own one pending job");
        job()
    }))
    .map_err(|_| RedThreadJoinError::Panicked);
    if let Ok(mut slot) = start.result.lock() {
        *slot = Some(outcome);
    }
    fusion_sys::thread::ThreadEntryReturn::new(0)
}

#[cfg(feature = "std")]
impl<T> RedThread<T>
where
    T: Send + 'static,
{
    /// Reports the current red-thread support surface.
    #[must_use]
    pub fn support() -> RedThreadSupport {
        red_thread_support()
    }

    /// Spawns one native urgent thread.
    ///
    /// # Errors
    ///
    /// Returns any honest unsupported policy or lower-level native thread creation failure.
    pub fn spawn<F>(config: &RedThreadConfig<'_>, job: F) -> Result<Self, ThreadError>
    where
        F: FnOnce() -> T + Send + 'static,
    {
        let mut admission = validate_red_thread_config(config)?;
        let result = std::sync::Arc::new(std::sync::Mutex::new(None));
        let start = std::boxed::Box::new(RedThreadStart {
            job: Some(job),
            result: std::sync::Arc::clone(&result),
        });
        let system = ThreadSystem::new();
        let context = std::boxed::Box::into_raw(start).cast::<()>();
        let handle =
            match unsafe { system.spawn_raw(&config.thread, red_thread_entry::<F, T>, context) } {
                Ok(handle) => handle,
                Err(error) => {
                    // SAFETY: the thread never started, so the boxed start record still belongs here.
                    unsafe {
                        drop(std::boxed::Box::from_raw(
                            context.cast::<RedThreadStart<F, T>>(),
                        ));
                    };
                    return Err(error);
                }
            };
        admission.placement = system
            .placement(&handle)
            .unwrap_or_else(|_| ThreadPlacementOutcome::unsupported());
        admission.scheduler = system
            .scheduler(&handle)
            .unwrap_or_else(|_| ThreadSchedulerObservation::unknown());
        Ok(Self {
            handle: Some(handle),
            result,
            admission,
        })
    }

    /// Returns the effective admission snapshot for this urgent thread.
    #[must_use]
    pub const fn admission(&self) -> RedThreadAdmission {
        self.admission
    }

    /// Waits for the urgent thread to complete and returns its result.
    ///
    /// # Errors
    ///
    /// Returns a native-thread failure or a panic marker when the entry unwound.
    pub fn join(mut self) -> Result<T, RedThreadJoinError> {
        let handle = self.handle.take().ok_or(ThreadError::state_conflict())?;
        let system = ThreadSystem::new();
        system.join(handle)?;
        let mut guard = self
            .result
            .lock()
            .map_err(|_| ThreadError::state_conflict())?;
        guard.take().ok_or(ThreadError::state_conflict())?
    }
}

#[cfg(feature = "std")]
impl<T> Drop for RedThread<T> {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = ThreadSystem::new().detach(handle);
        }
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    #[test]
    fn red_thread_spawn_runs_job_and_joins_result() {
        let thread = RedThread::spawn(&RedThreadConfig::new(), || 7_u8)
            .expect("hosted red thread spawn should succeed");
        assert_eq!(thread.join().expect("red thread join should succeed"), 7);
    }

    #[test]
    fn red_interrupt_default_uses_invalid_irq_sentinel() {
        assert_eq!(RedInterruptConfig::default().irqn, u16::MAX);
    }

    #[test]
    fn red_inline_compatibility_constructors_preserve_contract() {
        const SPAN: CooperativeExclusionSpan = match CooperativeExclusionSpan::new(3) {
            Ok(span) => span,
            Err(_) => panic!("test span should be valid"),
        };
        const SPANS: &[CooperativeExclusionSpan] = &[SPAN];
        const TREE_LEAF: [u32; 1] = [1_u32 << 2];
        const TREE_ROOT: [u32; 1] = [1_u32 << 0];
        const TREE_LEVELS: [&[u32]; 1] = [&TREE_ROOT];
        const TREE: CooperativeExclusionSummaryTree =
            CooperativeExclusionSummaryTree::new(&TREE_LEAF, &TREE_LEVELS);

        let from_spans = RedInlineCompatibility::from_spans(
            SPANS,
            VectorDispatchLane::DeferredPrimary,
            VectorDispatchCookie(11),
        )
        .with_current_exception_stack_bytes(64);
        assert!(matches!(
            from_spans.required_clear,
            RedInlineClearRequirement::Spans(required) if required == SPANS
        ));
        assert_eq!(from_spans.required_current_exception_stack_bytes, 64);
        assert_eq!(
            from_spans.fallback_lane,
            VectorDispatchLane::DeferredPrimary
        );
        assert_eq!(from_spans.fallback_cookie, VectorDispatchCookie(11));

        let from_tree = RedInlineCompatibility::from_summary_tree(
            &TREE,
            VectorDispatchLane::DeferredSecondary,
            VectorDispatchCookie(19),
        );
        assert!(matches!(
            from_tree.required_clear,
            RedInlineClearRequirement::SummaryTree(required) if core::ptr::eq(required, &TREE)
        ));
        assert_eq!(
            from_tree.fallback_lane,
            VectorDispatchLane::DeferredSecondary
        );
        assert_eq!(from_tree.fallback_cookie, VectorDispatchCookie(19));
    }
}
