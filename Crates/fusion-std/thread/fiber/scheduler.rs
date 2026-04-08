#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CarrierWaiterRecord {
    key: EventKey,
    source: EventSourceHandle,
    slot_index: usize,
    task_id: u64,
}

#[derive(Debug)]
struct CarrierReactorState {
    reactor: EventSystem,
    poller: SyncMutex<EventPoller>,
    waiters: SyncMutex<MetadataSlice<Option<CarrierWaiterRecord>>>,
    wake: PlatformFiberWakeSignal,
    wake_key: EventKey,
    capacity: PlatformFiberWakeSignal,
    capacity_key: EventKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct CarrierPollResult {
    ready_count: usize,
    capacity_signaled: bool,
}

#[cfg(feature = "std")]
#[derive(Debug)]
struct CarrierYieldBudgetState {
    slot_index: AtomicUsize,
    task_id: AtomicU64,
    budget_nanos: AtomicU64,
    started_nanos: AtomicU64,
    faulted: AtomicBool,
    reported: AtomicBool,
}

#[cfg(feature = "std")]
impl CarrierYieldBudgetState {
    const IDLE_SLOT: usize = usize::MAX;

    const fn new() -> Self {
        Self {
            slot_index: AtomicUsize::new(Self::IDLE_SLOT),
            task_id: AtomicU64::new(0),
            budget_nanos: AtomicU64::new(0),
            started_nanos: AtomicU64::new(0),
            faulted: AtomicBool::new(false),
            reported: AtomicBool::new(false),
        }
    }

    fn begin(&self, slot_index: usize, task_id: u64, start_nanos: u64, budget_nanos: u64) {
        self.task_id.store(task_id, Ordering::Release);
        self.started_nanos.store(start_nanos, Ordering::Release);
        self.budget_nanos.store(budget_nanos, Ordering::Release);
        self.faulted.store(false, Ordering::Release);
        self.reported.store(false, Ordering::Release);
        self.slot_index.store(slot_index, Ordering::Release);
    }

    fn clear(&self) {
        self.slot_index.store(Self::IDLE_SLOT, Ordering::Release);
        self.task_id.store(0, Ordering::Release);
        self.budget_nanos.store(0, Ordering::Release);
        self.started_nanos.store(0, Ordering::Release);
        self.faulted.store(false, Ordering::Release);
        self.reported.store(false, Ordering::Release);
    }
}

#[cfg(feature = "std")]
#[derive(Debug)]
struct GreenYieldBudgetRuntime {
    carriers: std::boxed::Box<[CarrierYieldBudgetState]>,
    watchdog_started: AtomicBool,
}

#[cfg(feature = "std")]
impl GreenYieldBudgetRuntime {
    fn new(carrier_count: usize) -> Self {
        let carriers = std::iter::repeat_with(CarrierYieldBudgetState::new)
            .take(carrier_count)
            .collect::<std::vec::Vec<_>>()
            .into_boxed_slice();
        Self {
            carriers,
            watchdog_started: AtomicBool::new(false),
        }
    }

    fn now_nanos() -> Result<u64, FiberError> {
        current_monotonic_nanos()
    }
}

impl CarrierReactorState {
    fn new(waiters: MetadataSlice<Option<CarrierWaiterRecord>>) -> Result<Self, FiberError> {
        for index in 0..waiters.len() {
            unsafe {
                waiters.write(index, None)?;
            }
        }

        let reactor = EventSystem::new();
        let host = system_fiber_host();
        let mut poller = reactor.create().map_err(fiber_error_from_event)?;
        let wake = host.create_wake_signal().map_err(fiber_error_from_host)?;
        let wake_key = reactor
            .register(
                &mut poller,
                EventSourceHandle(wake.source_handle().map_err(fiber_error_from_host)?),
                EventInterest::READABLE | EventInterest::ERROR | EventInterest::HANGUP,
            )
            .map_err(fiber_error_from_event)?;
        let capacity_signal = host.create_wake_signal().map_err(fiber_error_from_host)?;
        let capacity_key = reactor
            .register(
                &mut poller,
                EventSourceHandle(
                    capacity_signal
                        .source_handle()
                        .map_err(fiber_error_from_host)?,
                ),
                EventInterest::READABLE | EventInterest::ERROR | EventInterest::HANGUP,
            )
            .map_err(fiber_error_from_event)?;
        Ok(Self {
            reactor,
            poller: SyncMutex::new(poller),
            waiters: SyncMutex::new(waiters),
            wake,
            wake_key,
            capacity: capacity_signal,
            capacity_key,
        })
    }

    fn signal(&self) -> Result<(), FiberError> {
        self.wake.signal().map_err(fiber_error_from_host)
    }

    #[allow(clippy::missing_const_for_fn)]
    fn capacity_token(&self) -> PlatformWakeToken {
        self.capacity.token()
    }

    fn register_wait(
        &self,
        slot_index: usize,
        task_id: u64,
        source: EventSourceHandle,
        interest: EventInterest,
    ) -> Result<(), FiberError> {
        let mut poller = self.poller.lock().map_err(fiber_error_from_sync)?;
        let mut waiters = self.waiters.lock().map_err(fiber_error_from_sync)?;
        if waiters
            .iter()
            .flatten()
            .any(|waiter| waiter.source == source || waiter.slot_index == slot_index)
        {
            return Err(FiberError::state_conflict());
        }

        let slot = waiters
            .iter_mut()
            .find(|entry| entry.is_none())
            .ok_or_else(FiberError::resource_exhausted)?;
        let key = self
            .reactor
            .register(
                &mut poller,
                source,
                interest | EventInterest::ERROR | EventInterest::HANGUP,
            )
            .map_err(fiber_error_from_event)?;
        *slot = Some(CarrierWaiterRecord {
            key,
            source,
            slot_index,
            task_id,
        });
        Ok(())
    }

    fn waiter_count(&self) -> Result<usize, FiberError> {
        Ok(self
            .waiters
            .lock()
            .map_err(fiber_error_from_sync)?
            .iter()
            .flatten()
            .count())
    }

    fn poll_ready(
        &self,
        timeout: Option<Duration>,
        ready: &mut [Option<CarrierWaiterRecord>; CARRIER_EVENT_BATCH],
    ) -> Result<CarrierPollResult, FiberError> {
        let mut poller = self.poller.lock().map_err(fiber_error_from_sync)?;
        let mut events = [EMPTY_EVENT_RECORD; CARRIER_EVENT_BATCH];
        let count = self
            .reactor
            .poll(&mut poller, &mut events, timeout)
            .map_err(fiber_error_from_event)?;
        let mut result = CarrierPollResult::default();
        for event in events.into_iter().take(count) {
            if event.key == self.wake_key {
                self.wake.drain().map_err(fiber_error_from_host)?;
                continue;
            }
            if event.key == self.capacity_key {
                self.capacity.drain().map_err(fiber_error_from_host)?;
                result.capacity_signaled = true;
                continue;
            }

            let waiter = {
                let mut waiters = self.waiters.lock().map_err(fiber_error_from_sync)?;
                let slot = waiters
                    .iter_mut()
                    .find(|entry| entry.as_ref().is_some_and(|waiter| waiter.key == event.key));
                slot.and_then(Option::take)
            };

            if let Some(waiter) = waiter {
                self.reactor
                    .deregister(&mut poller, waiter.key)
                    .map_err(fiber_error_from_event)?;
                if result.ready_count < ready.len() {
                    ready[result.ready_count] = Some(waiter);
                    result.ready_count += 1;
                }
            }
        }
        Ok(result)
    }

    fn cancel_one_waiter(&self) -> Result<Option<CarrierWaiterRecord>, FiberError> {
        let mut poller = self.poller.lock().map_err(fiber_error_from_sync)?;
        let mut waiters = self.waiters.lock().map_err(fiber_error_from_sync)?;
        let Some(slot) = waiters.iter_mut().find(|entry| entry.is_some()) else {
            return Ok(None);
        };
        let waiter = slot.take().ok_or_else(FiberError::state_conflict)?;
        self.reactor
            .deregister(&mut poller, waiter.key)
            .map_err(fiber_error_from_event)?;
        Ok(Some(waiter))
    }
}

#[derive(Debug)]
struct CarrierQueue {
    queue: RuntimeCell<CarrierReadyQueue>,
    ready: Semaphore,
    reactor: Option<CarrierReactorState>,
    steal_state: AtomicUsize,
}

#[derive(Debug, Clone, Copy)]
struct CarrierQueueSlices {
    queue_entries: Option<MetadataSlice<usize>>,
    priority_buckets: Option<MetadataSlice<PriorityBucket>>,
    priority_next: Option<MetadataSlice<usize>>,
    priority_values: Option<MetadataSlice<i8>>,
    priority_enqueue_epochs: Option<MetadataSlice<u64>>,
    waiters: Option<MetadataSlice<Option<CarrierWaiterRecord>>>,
}

#[derive(Debug)]
enum CarrierReadyQueue {
    Fifo(MetadataIndexQueue),
    Priority(MetadataPriorityQueue),
}

impl CarrierReadyQueue {
    fn new(
        scheduling: GreenScheduling,
        slices: CarrierQueueSlices,
        priority_age_cap: Option<FiberTaskAgeCap>,
    ) -> Result<Self, FiberError> {
        match scheduling {
            GreenScheduling::Fifo | GreenScheduling::WorkStealing => Ok(Self::Fifo(
                MetadataIndexQueue::new(slices.queue_entries.ok_or_else(FiberError::invalid)?)?,
            )),
            GreenScheduling::Priority => Ok(Self::Priority(MetadataPriorityQueue::new(
                slices.priority_buckets.ok_or_else(FiberError::invalid)?,
                slices.priority_next.ok_or_else(FiberError::invalid)?,
                slices.priority_values.ok_or_else(FiberError::invalid)?,
                slices
                    .priority_enqueue_epochs
                    .ok_or_else(FiberError::invalid)?,
                priority_age_cap,
            )?)),
        }
    }

    fn enqueue(&mut self, value: usize, priority: FiberTaskPriority) -> Result<(), FiberError> {
        match self {
            Self::Fifo(queue) => queue.enqueue(value),
            Self::Priority(queue) => queue.enqueue(value, priority),
        }
    }

    fn dequeue(&mut self) -> Option<usize> {
        match self {
            Self::Fifo(queue) => queue.dequeue(),
            Self::Priority(queue) => queue.dequeue(),
        }
    }

    fn steal(&mut self) -> Option<usize> {
        match self {
            Self::Fifo(queue) => queue.steal(),
            Self::Priority(_) => None,
        }
    }
}

impl CarrierQueue {
    fn new(
        scheduling: GreenScheduling,
        slices: CarrierQueueSlices,
        priority_age_cap: Option<FiberTaskAgeCap>,
        seed: usize,
        fast: bool,
    ) -> Result<Self, FiberError> {
        let capacity = match scheduling {
            GreenScheduling::Fifo | GreenScheduling::WorkStealing => {
                slices.queue_entries.ok_or_else(FiberError::invalid)?.len()
            }
            GreenScheduling::Priority => {
                slices.priority_next.ok_or_else(FiberError::invalid)?.len()
            }
        };
        Ok(Self {
            queue: RuntimeCell::new(
                fast,
                CarrierReadyQueue::new(scheduling, slices, priority_age_cap)?,
            ),
            ready: Semaphore::new(
                0,
                u32::try_from(capacity).map_err(|_| FiberError::resource_exhausted())?,
            )
            .map_err(fiber_error_from_sync)?,
            reactor: match slices.waiters {
                Some(waiters) => Some(CarrierReactorState::new(waiters)?),
                None => None,
            },
            steal_state: AtomicUsize::new(seed.max(1)),
        })
    }

    fn signal(&self) -> Result<(), FiberError> {
        if let Some(reactor) = &self.reactor {
            return reactor.signal();
        }
        match self.ready.release(1) {
            Ok(()) => Ok(()),
            Err(error)
                if matches!(error.kind, SyncErrorKind::Overflow | SyncErrorKind::Invalid) =>
            {
                Ok(())
            }
            Err(error) => Err(fiber_error_from_sync(error)),
        }
    }

    fn capacity_token(&self) -> PlatformWakeToken {
        self.reactor.as_ref().map_or(
            PlatformWakeToken::invalid(),
            CarrierReactorState::capacity_token,
        )
    }

    fn next_steal_start(&self, carrier_count: usize) -> usize {
        if carrier_count <= 1 {
            return 0;
        }

        let mut current = self.steal_state.load(Ordering::Acquire).max(1);
        loop {
            let next = xorshift64(current);
            match self.steal_state.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    let peers = carrier_count - 1;
                    let offset = next % peers;
                    return offset + 1;
                }
                Err(observed) => current = observed.max(1),
            }
        }
    }
}

fn next_green_fiber_id() -> FiberId {
    static NEXT_GREEN_FIBER_ID: AtomicUsize = AtomicUsize::new(1);
    FiberId::new(NEXT_GREEN_FIBER_ID.fetch_add(1, Ordering::Relaxed))
}

#[derive(Debug)]
struct GreenTaskRecord {
    allocated: bool,
    id: u64,
    fiber_id: FiberId,
    class: fusion_sys::courier::CourierFiberClass,
    carrier: usize,
    stack_pool_index: usize,
    stack_slot: usize,
    stack_class: FiberStackClass,
    stack: Option<FiberStack>,
    priority: FiberTaskPriority,
    yield_budget: Option<Duration>,
    execution: FiberTaskExecution,
    fiber: Option<Fiber>,
    job: InlineGreenJobStorage,
    result: InlineGreenResultStorage,
    state: GreenTaskState,
}

impl GreenTaskRecord {
    const fn empty() -> Self {
        Self {
            allocated: false,
            id: 0,
            fiber_id: FiberId::new(0),
            class: fusion_sys::courier::CourierFiberClass::Dynamic,
            carrier: 0,
            stack_pool_index: 0,
            stack_slot: 0,
            stack_class: FiberStackClass::MIN,
            stack: None,
            priority: FiberTaskPriority::DEFAULT,
            yield_budget: None,
            execution: FiberTaskExecution::Fiber,
            fiber: None,
            job: InlineGreenJobStorage::empty(),
            result: InlineGreenResultStorage::empty(),
            state: GreenTaskState::Completed,
        }
    }
}

#[derive(Debug)]
struct GreenTaskSlot {
    owner: AtomicUsize,
    slot_index: usize,
    cooperative_lock_depth: AtomicUsize,
    cooperative_lock_ranks: [AtomicU16; MAX_COOPERATIVE_LOCK_NESTING],
    cooperative_exclusion_spans: [AtomicU16; MAX_COOPERATIVE_LOCK_NESTING],
    cooperative_exclusion_summary_leaf: [AtomicU32; ACTIVE_COOPERATIVE_EXCLUSION_FAST_LEAF_WORDS],
    cooperative_exclusion_summary_root: AtomicU32,
    cooperative_exclusion_summary_overflow: AtomicBool,
    completion_published: AtomicBool,
    completion_waiters: AtomicUsize,
    yield_action: RuntimeCell<CurrentGreenYieldAction>,
    record: RuntimeCell<GreenTaskRecord>,
    completed: RuntimeCell<Option<Semaphore>>,
    handle_refs: AtomicUsize,
}

impl GreenTaskSlot {
    fn new(slot_index: usize, fast: bool) -> Result<Self, FiberError> {
        Ok(Self {
            owner: AtomicUsize::new(0),
            slot_index,
            cooperative_lock_depth: AtomicUsize::new(0),
            cooperative_lock_ranks: [const { AtomicU16::new(UNRANKED_COOPERATIVE_LOCK) };
                MAX_COOPERATIVE_LOCK_NESTING],
            cooperative_exclusion_spans: [const { AtomicU16::new(NO_COOPERATIVE_EXCLUSION_SPAN) };
                MAX_COOPERATIVE_LOCK_NESTING],
            cooperative_exclusion_summary_leaf: [const { AtomicU32::new(0) };
                ACTIVE_COOPERATIVE_EXCLUSION_FAST_LEAF_WORDS],
            cooperative_exclusion_summary_root: AtomicU32::new(0),
            cooperative_exclusion_summary_overflow: AtomicBool::new(false),
            completion_published: AtomicBool::new(false),
            completion_waiters: AtomicUsize::new(0),
            yield_action: RuntimeCell::new(fast, CurrentGreenYieldAction::Requeue),
            record: RuntimeCell::new(fast, GreenTaskRecord::empty()),
            completed: RuntimeCell::new(fast, None),
            handle_refs: AtomicUsize::new(0),
        })
    }

    const fn context_ptr(&self) -> *mut () {
        core::ptr::from_ref(self).cast_mut().cast()
    }

    fn set_owner(&self, inner: *const GreenPoolInner) {
        self.owner.store(inner as usize, Ordering::Release);
    }

    fn current_context(&self) -> Result<CurrentGreenContext, FiberError> {
        let inner = self.owner.load(Ordering::Acquire) as *const GreenPoolInner;
        if inner.is_null() {
            return Err(FiberError::state_conflict());
        }

        Ok(CurrentGreenContext {
            inner,
            slot_index: self.slot_index,
            id: self.current_id()?,
            fiber_id: self.current_fiber_id()?,
            courier_id: unsafe { (*inner).courier_id },
            context_id: unsafe { (*inner).context_id },
        })
    }

    fn set_yield_action(&self, action: CurrentGreenYieldAction) -> Result<(), FiberError> {
        self.yield_action
            .with(|yield_action| *yield_action = action)?;
        Ok(())
    }

    fn enter_cooperative_lock(
        &self,
        rank: Option<u16>,
        span: Option<CooperativeExclusionSpan>,
    ) -> Result<CooperativeGreenLockToken, SyncError> {
        let depth = self.cooperative_lock_depth.load(Ordering::Acquire);
        if depth >= MAX_COOPERATIVE_LOCK_NESTING {
            return Err(SyncError::overflow());
        }

        let rank_value = rank.unwrap_or(UNRANKED_COOPERATIVE_LOCK);
        if depth != 0 {
            let current_rank = self.cooperative_lock_ranks[depth - 1].load(Ordering::Acquire);
            if current_rank != UNRANKED_COOPERATIVE_LOCK
                && rank_value != UNRANKED_COOPERATIVE_LOCK
                && rank_value <= current_rank
            {
                return Err(SyncError::invalid());
            }
        }

        self.cooperative_lock_ranks[depth].store(rank_value, Ordering::Release);
        self.cooperative_exclusion_spans[depth].store(
            span.map_or(NO_COOPERATIVE_EXCLUSION_SPAN, CooperativeExclusionSpan::get),
            Ordering::Release,
        );
        self.cooperative_lock_depth
            .store(depth + 1, Ordering::Release);
        self.rebuild_cooperative_exclusion_summary_tree(depth + 1);
        Ok(CooperativeGreenLockToken {
            slot: core::ptr::from_ref(self).cast(),
            depth_index: depth,
        })
    }

    fn exit_cooperative_lock(&self, depth_index: usize) {
        let previous = self.cooperative_lock_depth.load(Ordering::Acquire);
        assert!(
            previous > 0,
            "cooperative green lock depth underflow indicates unbalanced guard bookkeeping"
        );
        assert_eq!(
            previous,
            depth_index + 1,
            "cooperative green locks should release in reverse acquisition order"
        );
        self.cooperative_lock_ranks[depth_index]
            .store(UNRANKED_COOPERATIVE_LOCK, Ordering::Release);
        self.cooperative_exclusion_spans[depth_index]
            .store(NO_COOPERATIVE_EXCLUSION_SPAN, Ordering::Release);
        self.cooperative_lock_depth
            .store(depth_index, Ordering::Release);
        self.rebuild_cooperative_exclusion_summary_tree(depth_index);
    }

    fn reset_cooperative_lock_depth(&self) {
        self.cooperative_lock_depth.store(0, Ordering::Release);
        for rank in &self.cooperative_lock_ranks {
            rank.store(UNRANKED_COOPERATIVE_LOCK, Ordering::Release);
        }
        for span in &self.cooperative_exclusion_spans {
            span.store(NO_COOPERATIVE_EXCLUSION_SPAN, Ordering::Release);
        }
        for word in &self.cooperative_exclusion_summary_leaf {
            word.store(0, Ordering::Release);
        }
        self.cooperative_exclusion_summary_root
            .store(0, Ordering::Release);
        self.cooperative_exclusion_summary_overflow
            .store(false, Ordering::Release);
    }

    fn cooperative_lock_depth(&self) -> usize {
        self.cooperative_lock_depth.load(Ordering::Acquire)
    }

    fn copy_active_exclusion_spans(&self, output: &mut [CooperativeExclusionSpan]) -> usize {
        let depth = self.cooperative_lock_depth();
        let mut written = 0;
        for index in 0..depth {
            if written >= output.len() {
                break;
            }
            let raw = self.cooperative_exclusion_spans[index].load(Ordering::Acquire);
            let Some(span) = NonZeroU16::new(raw).map(CooperativeExclusionSpan) else {
                continue;
            };
            output[written] = span;
            written += 1;
        }
        written
    }

    fn rebuild_cooperative_exclusion_summary_tree(&self, depth: usize) {
        let mut leaf = [0_u32; ACTIVE_COOPERATIVE_EXCLUSION_FAST_LEAF_WORDS];
        let mut root = 0_u32;
        let mut overflow = false;

        for index in 0..depth {
            let raw = self.cooperative_exclusion_spans[index].load(Ordering::Acquire);
            let Some(span) = NonZeroU16::new(raw) else {
                continue;
            };
            let span_index = usize::from(span.get() - 1);
            if span_index >= ACTIVE_COOPERATIVE_EXCLUSION_FAST_SPAN_CAPACITY {
                overflow = true;
                continue;
            }
            let word_index = span_index / COOPERATIVE_EXCLUSION_TREE_WORD_BITS;
            let bit = 1_u32 << (span_index % COOPERATIVE_EXCLUSION_TREE_WORD_BITS);
            leaf[word_index] |= bit;
            root |= 1_u32 << word_index;
        }

        for (index, word) in leaf.into_iter().enumerate() {
            self.cooperative_exclusion_summary_leaf[index].store(word, Ordering::Release);
        }
        self.cooperative_exclusion_summary_root
            .store(root, Ordering::Release);
        self.cooperative_exclusion_summary_overflow
            .store(overflow, Ordering::Release);
    }

    fn exclusion_summary_tree_allows(&self, tree: &CooperativeExclusionSummaryTree) -> bool {
        if tree.leaf_words.is_empty() {
            return true;
        }

        if !self
            .cooperative_exclusion_summary_overflow
            .load(Ordering::Acquire)
            && tree.leaf_words.len() <= ACTIVE_COOPERATIVE_EXCLUSION_FAST_LEAF_WORDS
        {
            match tree.summary_levels {
                [] if tree.leaf_words.len() == 1 => {
                    return self.cooperative_exclusion_summary_leaf[0].load(Ordering::Acquire)
                        & tree.leaf_words[0]
                        == 0;
                }
                [root] if root.len() == 1 => {
                    let overlap = self
                        .cooperative_exclusion_summary_root
                        .load(Ordering::Acquire)
                        & root[0];
                    if overlap == 0 {
                        return true;
                    }
                    let mut bits = overlap;
                    while bits != 0 {
                        let leaf_index = bits.trailing_zeros() as usize;
                        if self.cooperative_exclusion_summary_leaf[leaf_index]
                            .load(Ordering::Acquire)
                            & tree.leaf_words[leaf_index]
                            != 0
                        {
                            return false;
                        }
                        bits &= bits - 1;
                    }
                    return true;
                }
                _ => {}
            }
        }

        let depth = self.cooperative_lock_depth();
        for index in 0..depth {
            let raw = self.cooperative_exclusion_spans[index].load(Ordering::Acquire);
            let Some(active) = NonZeroU16::new(raw).map(CooperativeExclusionSpan) else {
                continue;
            };
            if tree.contains(active) {
                return false;
            }
        }
        true
    }

    fn take_yield_action(&self) -> Result<CurrentGreenYieldAction, FiberError> {
        self.yield_action
            .with(|yield_action| core::mem::replace(yield_action, CurrentGreenYieldAction::Requeue))
    }

    fn assign<F>(
        &self,
        id: u64,
        fiber_id: FiberId,
        class: fusion_sys::courier::CourierFiberClass,
        carrier: usize,
        lease: Option<FiberStackLease>,
        task: FiberTaskAttributes,
        job: F,
    ) -> Result<(), FiberError>
    where
        F: FnOnce() + Send + 'static,
    {
        self.completed.with_ref(|completed| {
            if let Some(semaphore) = completed.as_ref() {
                while semaphore.try_acquire().map_err(fiber_error_from_sync)? {}
            }
            Ok::<(), FiberError>(())
        })??;

        self.record.with(|record| {
            if record.allocated {
                return Err(FiberError::state_conflict());
            }

            record.job.clear();
            record.result.clear();
            record.job.store(job)?;
            record.allocated = true;
            record.id = id;
            record.fiber_id = fiber_id;
            record.class = class;
            record.carrier = carrier;
            record.stack_pool_index = lease.map_or(0, |reserved| reserved.pool_index);
            record.stack_slot = lease.map_or(0, |reserved| reserved.slot_index);
            record.stack_class = lease.map_or(task.stack_class, |reserved| reserved.class);
            record.stack = lease.map(|reserved| reserved.stack);
            record.priority = task.priority;
            record.yield_budget = task.yield_budget;
            record.execution = task.execution;
            record.fiber = None;
            record.state = GreenTaskState::Queued;
            Ok(())
        })??;
        self.completion_published.store(false, Ordering::Release);
        self.completion_waiters.store(0, Ordering::Release);
        self.handle_refs.store(1, Ordering::Release);
        self.reset_cooperative_lock_depth();
        Ok(())
    }

    fn clone_handle(&self) {
        self.handle_refs.fetch_add(1, Ordering::AcqRel);
    }

    fn current_id(&self) -> Result<u64, FiberError> {
        self.record.with_ref(|record| {
            if !record.allocated {
                return Err(FiberError::state_conflict());
            }
            Ok(record.id)
        })?
    }

    fn current_fiber_id(&self) -> Result<FiberId, FiberError> {
        self.record.with_ref(|record| {
            if !record.allocated {
                return Err(FiberError::state_conflict());
            }
            Ok(record.fiber_id)
        })?
    }

    fn priority(&self) -> Result<FiberTaskPriority, FiberError> {
        self.record.with_ref(|record| {
            if !record.allocated {
                return Err(FiberError::state_conflict());
            }
            Ok(record.priority)
        })?
    }

    fn execution(&self) -> Result<FiberTaskExecution, FiberError> {
        self.record.with_ref(|record| {
            if !record.allocated {
                return Err(FiberError::state_conflict());
            }
            Ok(record.execution)
        })?
    }

    fn execution_for(&self, id: u64) -> Result<FiberTaskExecution, FiberError> {
        self.record.with_ref(|record| {
            if !Self::matches_id(record, id) {
                return Err(FiberError::state_conflict());
            }
            Ok(record.execution)
        })?
    }

    fn admission_for(&self, id: u64) -> Result<FiberTaskAdmission, FiberError> {
        self.record.with_ref(|record| {
            if !Self::matches_id(record, id) {
                return Err(FiberError::state_conflict());
            }
            Ok(FiberTaskAdmission {
                carrier: record.carrier,
                stack_class: record.stack_class,
                priority: record.priority,
                yield_budget: record.yield_budget,
                execution: record.execution,
            })
        })?
    }

    const fn matches_id(record: &GreenTaskRecord, id: u64) -> bool {
        record.allocated && record.id == id
    }

    fn install_fiber(&self, id: u64, fiber: Fiber) -> Result<(), FiberError> {
        self.record.with(|record| {
            if !Self::matches_id(record, id) {
                return Err(FiberError::state_conflict());
            }
            record.stack = None;
            record.fiber = Some(fiber);
            Ok(())
        })??;
        Ok(())
    }

    fn materialize_fiber(
        &self,
        id: u64,
        entry: FiberEntry,
        arg: *mut (),
    ) -> Result<(), FiberError> {
        self.record.with(|record| {
            if !Self::matches_id(record, id) {
                return Err(FiberError::state_conflict());
            }
            if record.fiber.is_some() {
                return Ok(());
            }
            let stack = record.stack.take().ok_or_else(FiberError::state_conflict)?;
            let fiber = Fiber::new(stack, entry, arg)?;
            record.fiber = Some(fiber);
            Ok(())
        })?
    }

    fn clear_fiber(&self, id: u64) -> Result<(), FiberError> {
        self.record.with(|record| {
            if !Self::matches_id(record, id) {
                return Err(FiberError::state_conflict());
            }
            record.fiber = None;
            Ok(())
        })??;
        Ok(())
    }

    fn stack_location(&self, id: u64) -> Result<(usize, usize), FiberError> {
        self.record.with_ref(|record| {
            if !Self::matches_id(record, id) {
                return Err(FiberError::state_conflict());
            }
            Ok((record.stack_pool_index, record.stack_slot))
        })?
    }

    fn assignment(&self) -> Result<Option<(u64, usize)>, FiberError> {
        self.record.with_ref(|record| {
            if !record.allocated {
                return Ok(None);
            }
            Ok(Some((record.id, record.carrier)))
        })?
    }

    fn reassign_carrier(&self, id: u64, carrier: usize) -> Result<(), FiberError> {
        self.record.with(|record| {
            if !Self::matches_id(record, id) {
                return Err(FiberError::state_conflict());
            }
            if matches!(
                record.state,
                GreenTaskState::Running | GreenTaskState::Waiting | GreenTaskState::Finishing
            ) {
                return Err(FiberError::state_conflict());
            }
            record.carrier = carrier;
            Ok(())
        })??;
        Ok(())
    }

    fn state(&self, id: u64) -> Result<GreenTaskState, FiberError> {
        self.record.with_ref(|record| {
            if !Self::matches_id(record, id) {
                return Err(FiberError::state_conflict());
            }
            Ok(record.state)
        })?
    }

    fn is_finished(&self, id: u64) -> Result<bool, FiberError> {
        Ok(is_terminal_task_state(self.state(id)?)
            && self.completion_published.load(Ordering::Acquire))
    }

    fn ensure_completion_semaphore(&self) -> Result<*const Semaphore, FiberError> {
        self.completed.with(|completed| {
            if completed.is_none() {
                *completed = Some(Semaphore::new(0, 1).map_err(fiber_error_from_sync)?);
            }
            Ok::<*const Semaphore, FiberError>(core::ptr::from_ref(
                completed.as_ref().ok_or_else(FiberError::state_conflict)?,
            ))
        })?
    }

    fn wait_until_terminal(&self, id: u64) -> Result<GreenTaskState, FiberError> {
        let waited = if self.is_finished(id)? {
            false
        } else {
            let semaphore = self.ensure_completion_semaphore()?;
            self.completion_waiters.fetch_add(1, Ordering::AcqRel);
            if self.is_finished(id)? {
                self.completion_waiters.fetch_sub(1, Ordering::AcqRel);
                false
            } else {
                unsafe { &*semaphore }
                    .acquire()
                    .map_err(fiber_error_from_sync)?;
                true
            }
        };

        let state = self.state(id)?;
        if waited {
            let remaining_waiters = self
                .completion_waiters
                .fetch_sub(1, Ordering::AcqRel)
                .saturating_sub(1);
            if is_terminal_task_state(state) && remaining_waiters != 0 {
                self.completed.with_ref(|completed| {
                    completed
                        .as_ref()
                        .ok_or_else(FiberError::state_conflict)?
                        .release(1)
                        .map_err(fiber_error_from_sync)
                })??;
            }
        }
        Ok(state)
    }

    fn set_state(&self, id: u64, state: GreenTaskState) -> Result<(), FiberError> {
        self.record.with(|record| {
            if !Self::matches_id(record, id) {
                return Err(FiberError::state_conflict());
            }
            record.state = state;
            Ok(())
        })??;
        Ok(())
    }

    fn signal_completed(&self, id: u64) -> Result<(), FiberError> {
        self.record.with_ref(|record| {
            if !Self::matches_id(record, id) || !is_terminal_task_state(record.state) {
                #[cfg(feature = "std")]
                if std::env::var_os("FUSION_TRACE_CARRIER_ERRORS").is_some() {
                    std::eprintln!(
                        "fusion-std signal_completed mismatch: slot_index={} expected_id={} actual_id={} allocated={} state={:?}",
                        self.slot_index,
                        id,
                        record.id,
                        record.allocated,
                        record.state,
                    );
                }
                return Err(FiberError::state_conflict());
            }
            Ok(())
        })??;
        let release = self.completed.with_ref(|completed| {
            completed
                .as_ref()
                .ok_or_else(FiberError::state_conflict)?
                .release(1)
                .map_err(fiber_error_from_sync)
        })?;
        self.completion_published.store(true, Ordering::Release);
        match release {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == FiberError::state_conflict().kind() => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn resume(&self, id: u64) -> Result<FiberYield, FiberError> {
        let mut fiber = {
            self.record.with(|record| {
                if !Self::matches_id(record, id) {
                    return Err(FiberError::state_conflict());
                }
                record.fiber.take().ok_or_else(FiberError::state_conflict)
            })??
        };

        let inner = self.owner.load(Ordering::Acquire) as *const GreenPoolInner;
        if inner.is_null() {
            return Err(FiberError::state_conflict());
        }
        let fiber_id = self.current_fiber_id()?;
        let courier_id = unsafe { (*inner).courier_id };
        let context_id = unsafe { (*inner).context_id };

        match fiber.resume_bound(Some(fiber_id), courier_id, context_id) {
            Ok(FiberYield::Yielded) => {
                self.record.with(|record| {
                    if !Self::matches_id(record, id) {
                        return Err(FiberError::state_conflict());
                    }
                    record.fiber = Some(fiber);
                    Ok(())
                })??;
                Ok(FiberYield::Yielded)
            }
            Ok(FiberYield::Completed(result)) => Ok(FiberYield::Completed(result)),
            Err(error) => Err(error),
        }
    }

    fn take_job_runner(&self, id: u64) -> Result<InlineGreenJobRunner, FiberError> {
        self.record.with(|record| {
            if !Self::matches_id(record, id) {
                return Err(FiberError::state_conflict());
            }
            record.job.take_runner()
        })?
    }

    fn store_output<T: 'static>(&self, id: u64, value: T) -> Result<(), FiberError> {
        self.record.with(|record| {
            if !Self::matches_id(record, id) {
                return Err(FiberError::state_conflict());
            }
            record.result.store(value)
        })?
    }

    fn take_output<T: 'static>(&self, id: u64) -> Result<T, FiberError> {
        self.record.with(|record| {
            if !Self::matches_id(record, id) {
                return Err(FiberError::state_conflict());
            }
            record.result.take::<T>()
        })?
    }

    fn force_recycle(&self, id: u64) -> Result<bool, FiberError> {
        let recycled = self.record.with(|record| {
            if !Self::matches_id(record, id) {
                return Ok::<bool, FiberError>(false);
            }
            record.job.clear();
            record.result.clear();
            record.fiber = None;
            record.allocated = false;
            record.id = 0;
            record.carrier = 0;
            record.stack_pool_index = 0;
            record.stack_slot = 0;
            record.stack_class = FiberStackClass::MIN;
            record.stack = None;
            record.priority = FiberTaskPriority::DEFAULT;
            record.yield_budget = None;
            record.execution = FiberTaskExecution::Fiber;
            record.state = GreenTaskState::Completed;
            Ok::<bool, FiberError>(true)
        })??;
        if recycled {
            #[cfg(feature = "std")]
            if std::env::var_os("FUSION_TRACE_CARRIER_ERRORS").is_some() {
                std::eprintln!(
                    "fusion-std force_recycle: slot_index={} id={}",
                    self.slot_index,
                    id
                );
            }
            self.completion_published.store(false, Ordering::Release);
            self.completion_waiters.store(0, Ordering::Release);
            self.handle_refs.store(0, Ordering::Release);
            self.reset_cooperative_lock_depth();
        }
        Ok(recycled)
    }

    fn try_recycle(&self, id: u64) -> Result<bool, FiberError> {
        let recycled = self.record.with(|record| {
            if !Self::matches_id(record, id) {
                return Ok::<bool, FiberError>(false);
            }
            if !is_terminal_task_state(record.state)
                || !self.completion_published.load(Ordering::Acquire)
                || self.handle_refs.load(Ordering::Acquire) != 0
            {
                return Ok::<bool, FiberError>(false);
            }
            record.job.clear();
            record.result.clear();
            record.fiber = None;
            record.allocated = false;
            record.id = 0;
            record.carrier = 0;
            record.stack_pool_index = 0;
            record.stack_slot = 0;
            record.stack_class = FiberStackClass::MIN;
            record.stack = None;
            record.priority = FiberTaskPriority::DEFAULT;
            record.yield_budget = None;
            record.execution = FiberTaskExecution::Fiber;
            record.state = GreenTaskState::Completed;
            Ok::<bool, FiberError>(true)
        })??;
        if recycled {
            #[cfg(feature = "std")]
            if std::env::var_os("FUSION_TRACE_CARRIER_ERRORS").is_some() {
                std::eprintln!(
                    "fusion-std try_recycle: slot_index={} id={}",
                    self.slot_index,
                    id
                );
            }
            self.completion_published.store(false, Ordering::Release);
            self.completion_waiters.store(0, Ordering::Release);
            self.reset_cooperative_lock_depth();
        }
        Ok(recycled)
    }

    fn begin_run(&self) -> Result<(u64, Option<Duration>, FiberTaskExecution), FiberError> {
        self.record.with(|record| {
            if !record.allocated {
                return Err(FiberError::state_conflict());
            }
            let task_id = record.id;
            let yield_budget = record.yield_budget;
            let execution = record.execution;
            record.state = GreenTaskState::Running;
            Ok((task_id, yield_budget, execution))
        })?
    }

    fn settle_terminal_state(&self, id: u64, terminal: GreenTaskState) -> Result<(), FiberError> {
        self.record.with(|record| {
            if !Self::matches_id(record, id) {
                return Err(FiberError::state_conflict());
            }
            if !matches!(record.state, GreenTaskState::Failed(_)) {
                record.state = terminal;
            }
            Ok(())
        })??;
        Ok(())
    }

    fn begin_finish(
        &self,
        id: u64,
        terminal: GreenTaskState,
    ) -> Result<GreenTaskState, FiberError> {
        self.record.with(|record| {
            if !Self::matches_id(record, id) {
                return Err(FiberError::state_conflict());
            }
            let resolved = if let GreenTaskState::Failed(error) = record.state {
                GreenTaskState::Failed(error)
            } else {
                terminal
            };
            record.state = GreenTaskState::Finishing;
            Ok(resolved)
        })?
    }
}

#[derive(Debug)]
struct GreenTaskRegistry {
    slots: MetadataSlice<GreenTaskSlot>,
    free: RuntimeCell<MetadataIndexStack>,
}

impl GreenTaskRegistry {
    fn new(
        slots: MetadataSlice<GreenTaskSlot>,
        free_entries: MetadataSlice<usize>,
        fast: bool,
    ) -> Result<Self, FiberError> {
        if slots.is_empty() || slots.len() != free_entries.len() {
            return Err(FiberError::invalid());
        }

        for slot_index in 0..slots.len() {
            unsafe {
                slots.write(slot_index, GreenTaskSlot::new(slot_index, fast)?)?;
            }
        }

        Ok(Self {
            free: RuntimeCell::new(
                fast,
                MetadataIndexStack::with_prefix(free_entries, slots.len())?,
            ),
            slots,
        })
    }

    fn reserve_slot(&self) -> Result<usize, FiberError> {
        self.free
            .with(|free| free.pop().ok_or_else(FiberError::resource_exhausted))?
    }

    fn available_slots(&self) -> Result<usize, FiberError> {
        self.free.with_ref(|free| free.len)
    }

    fn lane_counts(&self) -> Result<(usize, usize, usize, usize), FiberError> {
        let mut active = 0usize;
        let mut runnable = 0usize;
        let mut running = 0usize;
        let mut blocked = 0usize;
        for slot in &*self.slots {
            slot.record.with_ref(|record| {
                if !record.allocated {
                    return Ok::<(), FiberError>(());
                }
                active = active.saturating_add(1);
                match record.state {
                    GreenTaskState::Queued | GreenTaskState::Yielded => {
                        runnable = runnable.saturating_add(1);
                    }
                    GreenTaskState::Running | GreenTaskState::Finishing => {
                        running = running.saturating_add(1);
                    }
                    GreenTaskState::Waiting => {
                        blocked = blocked.saturating_add(1);
                    }
                    GreenTaskState::Completed | GreenTaskState::Failed(_) => {}
                }
                Ok(())
            })??;
        }
        Ok((active, runnable, running, blocked))
    }

    fn initialize_owner(&self, inner: *const GreenPoolInner) {
        for slot in &*self.slots {
            slot.set_owner(inner);
        }
    }

    fn assign_job<F>(
        &self,
        slot_index: usize,
        id: u64,
        fiber_id: FiberId,
        class: fusion_sys::courier::CourierFiberClass,
        carrier: usize,
        lease: Option<FiberStackLease>,
        task: FiberTaskAttributes,
        job: F,
    ) -> Result<(), FiberError>
    where
        F: FnOnce() + Send + 'static,
    {
        let slot = &self.slots[slot_index];
        slot.assign(id, fiber_id, class, carrier, lease, task, job)
    }

    fn recycle_slot(&self, slot_index: usize) -> Result<(), FiberError> {
        self.free.with(|free| free.push(slot_index))?
    }

    fn slot(&self, slot_index: usize) -> Result<&GreenTaskSlot, FiberError> {
        self.slots.get(slot_index).ok_or_else(FiberError::invalid)
    }

    fn slot_context(&self, slot_index: usize) -> Result<*mut (), FiberError> {
        Ok(self.slot(slot_index)?.context_ptr())
    }

    fn clone_handle(&self, slot_index: usize) -> Result<(), FiberError> {
        self.slot(slot_index)?.clone_handle();
        Ok(())
    }

    fn current_id(&self, slot_index: usize) -> Result<u64, FiberError> {
        self.slot(slot_index)?.current_id()
    }

    fn current_fiber_id(&self, slot_index: usize) -> Result<FiberId, FiberError> {
        self.slot(slot_index)?.current_fiber_id()
    }

    fn priority(&self, slot_index: usize) -> Result<FiberTaskPriority, FiberError> {
        self.slot(slot_index)?.priority()
    }

    fn execution(&self, slot_index: usize) -> Result<FiberTaskExecution, FiberError> {
        self.slot(slot_index)?.execution()
    }

    fn execution_for(&self, slot_index: usize, id: u64) -> Result<FiberTaskExecution, FiberError> {
        self.slot(slot_index)?.execution_for(id)
    }

    fn admission_for(&self, slot_index: usize, id: u64) -> Result<FiberTaskAdmission, FiberError> {
        self.slot(slot_index)?.admission_for(id)
    }

    fn install_fiber(&self, slot_index: usize, id: u64, fiber: Fiber) -> Result<(), FiberError> {
        self.slot(slot_index)?.install_fiber(id, fiber)
    }

    fn materialize_fiber(
        &self,
        slot_index: usize,
        id: u64,
        entry: FiberEntry,
        arg: *mut (),
    ) -> Result<(), FiberError> {
        self.slot(slot_index)?.materialize_fiber(id, entry, arg)
    }

    fn clear_fiber(&self, slot_index: usize, id: u64) -> Result<(), FiberError> {
        self.slot(slot_index)?.clear_fiber(id)
    }

    fn stack_location(&self, slot_index: usize, id: u64) -> Result<(usize, usize), FiberError> {
        self.slot(slot_index)?.stack_location(id)
    }

    fn assignment(&self, slot_index: usize) -> Result<Option<(u64, usize)>, FiberError> {
        self.slot(slot_index)?.assignment()
    }

    fn reassign_carrier(
        &self,
        slot_index: usize,
        id: u64,
        carrier: usize,
    ) -> Result<(), FiberError> {
        self.slot(slot_index)?.reassign_carrier(id, carrier)
    }

    fn state(&self, slot_index: usize, id: u64) -> Result<GreenTaskState, FiberError> {
        self.slot(slot_index)?.state(id)
    }

    fn is_finished(&self, slot_index: usize, id: u64) -> Result<bool, FiberError> {
        self.slot(slot_index)?.is_finished(id)
    }

    fn wait_until_terminal(
        &self,
        slot_index: usize,
        id: u64,
    ) -> Result<GreenTaskState, FiberError> {
        self.slot(slot_index)?.wait_until_terminal(id)
    }

    fn set_state(
        &self,
        slot_index: usize,
        id: u64,
        state: GreenTaskState,
    ) -> Result<(), FiberError> {
        self.slot(slot_index)?.set_state(id, state)
    }

    fn signal_completed(&self, slot_index: usize, id: u64) -> Result<(), FiberError> {
        self.slot(slot_index)?.signal_completed(id)
    }

    fn resume(&self, slot_index: usize, id: u64) -> Result<FiberYield, FiberError> {
        self.slot(slot_index)?.resume(id)
    }

    fn take_output<T: 'static>(&self, slot_index: usize, id: u64) -> Result<T, FiberError> {
        self.slot(slot_index)?.take_output::<T>(id)
    }

    fn release_handle(&self, slot_index: usize, id: u64) -> Result<(), FiberError> {
        let slot = self.slot(slot_index)?;
        let previous = slot
            .handle_refs
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current: usize| {
                current.checked_sub(1)
            })
            .map_err(|_| FiberError::state_conflict())?;
        #[cfg(feature = "std")]
        if std::env::var_os("FUSION_TRACE_CARRIER_ERRORS").is_some() {
            std::eprintln!(
                "fusion-std release_handle: slot_index={slot_index} id={id} previous_refs={previous}"
            );
        }
        if previous == 1 && slot.try_recycle(id)? {
            self.recycle_slot(slot_index)?;
        }
        Ok(())
    }

    fn try_reclaim(&self, slot_index: usize, id: u64) -> Result<(), FiberError> {
        if self.slot(slot_index)?.try_recycle(id)? {
            self.recycle_slot(slot_index)?;
        }
        Ok(())
    }

    fn abandon(&self, slot_index: usize, id: u64) -> Result<(), FiberError> {
        if self.slot(slot_index)?.force_recycle(id)? {
            self.recycle_slot(slot_index)?;
        }
        Ok(())
    }
}
