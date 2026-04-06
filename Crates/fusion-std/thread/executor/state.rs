struct ExecutorCell<T> {
    fast: bool,
    value: UnsafeCell<T>,
    lock: SysMutex<()>,
}

unsafe impl<T: Send> Send for ExecutorCell<T> {}
unsafe impl<T: Send> Sync for ExecutorCell<T> {}

impl<T: fmt::Debug> fmt::Debug for ExecutorCell<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExecutorCell")
            .field("fast", &self.fast)
            .finish_non_exhaustive()
    }
}

impl<T> ExecutorCell<T> {
    const fn new(fast: bool, value: T) -> Self {
        Self {
            fast,
            value: UnsafeCell::new(value),
            lock: SysMutex::new(()),
        }
    }

    fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> Result<R, ExecutorError> {
        if self.fast {
            // SAFETY: fast-mode cells are only installed by the thread-affine current runtime.
            return Ok(unsafe { f(&mut *self.value.get()) });
        }
        let _guard = self.lock.lock().map_err(executor_error_from_sync)?;
        // SAFETY: the lock serializes mutable access in shared modes.
        Ok(unsafe { f(&mut *self.value.get()) })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn with_ref<R>(&self, f: impl FnOnce(&T) -> R) -> Result<R, ExecutorError> {
        if self.fast {
            // SAFETY: fast-mode cells are only installed by the thread-affine current runtime.
            return Ok(unsafe { f(&*self.value.get()) });
        }
        let _guard = self.lock.lock().map_err(executor_error_from_sync)?;
        // SAFETY: the lock serializes shared access in shared modes.
        Ok(unsafe { f(&*self.value.get()) })
    }
}

struct CurrentQueue {
    ready: ExecutorCell<CurrentQueueState>,
}

struct ExecutorReactorState {
    poller: ExecutorCell<Option<ReactorPoller>>,
    events: ExecutorCell<[EventRecord; REACTOR_EVENT_BATCH]>,
    waits: ExecutorCell<ArenaSlice<AsyncReactorWaitEntry>>,
    outcomes: ExecutorCell<ArenaSlice<Option<AsyncWaitOutcome>>>,
    #[cfg(feature = "std")]
    pending_deregister: ExecutorCell<ArenaSlice<Option<EventKey>>>,
    #[cfg(feature = "std")]
    wake: ExecutorCell<Option<ExecutorReactorWakeSignal>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum AsyncReactorWaitKind {
    None,
    #[cfg(feature = "std")]
    ReadinessPending {
        generation: u64,
        source: EventSourceHandle,
        interest: EventInterest,
    },
    ReadinessRegistered {
        generation: u64,
        key: EventKey,
    },
    Sleep {
        generation: u64,
        deadline: CanonicalInstant,
        raw_deadline: Option<MonotonicRawInstant>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct AsyncReactorWaitEntry {
    kind: AsyncReactorWaitKind,
}

impl AsyncReactorWaitEntry {
    const EMPTY: Self = Self {
        kind: AsyncReactorWaitKind::None,
    };

    const fn readiness(generation: u64, key: EventKey) -> Self {
        Self {
            kind: AsyncReactorWaitKind::ReadinessRegistered { generation, key },
        }
    }

    #[cfg(feature = "std")]
    const fn readiness_pending(
        generation: u64,
        source: EventSourceHandle,
        interest: EventInterest,
    ) -> Self {
        Self {
            kind: AsyncReactorWaitKind::ReadinessPending {
                generation,
                source,
                interest,
            },
        }
    }

    const fn sleep(
        generation: u64,
        deadline: CanonicalInstant,
        raw_deadline: Option<MonotonicRawInstant>,
    ) -> Self {
        Self {
            kind: AsyncReactorWaitKind::Sleep {
                generation,
                deadline,
                raw_deadline,
            },
        }
    }
}

#[cfg(feature = "std")]
struct ExecutorReactorWakeSignal {
    signal: PlatformFiberWakeSignal,
    key: Option<EventKey>,
}

#[derive(Debug, Clone, Copy)]
struct CurrentJob {
    run: unsafe fn(usize, usize, u64),
    core: usize,
    slot_index: usize,
    generation: u64,
}

#[derive(Debug)]
struct CurrentQueueState {
    entries: ArenaSlice<Option<CurrentJob>>,
    head: usize,
    tail: usize,
    len: usize,
}

#[cfg(feature = "std")]
#[derive(Debug)]
struct HostedReadyQueueState {
    entries: [Option<CurrentJob>; CURRENT_QUEUE_CAPACITY],
    head: usize,
    tail: usize,
    len: usize,
}
impl CurrentQueue {
    fn new_in(arena: &BoundedArena, capacity: usize, fast: bool) -> Result<Self, ExecutorError> {
        let entries = arena
            .alloc_array_with(capacity.max(1), |_| None)
            .map_err(executor_error_from_alloc)?;
        Ok(Self {
            ready: ExecutorCell::new(
                fast,
                CurrentQueueState {
                    entries,
                    head: 0,
                    tail: 0,
                    len: 0,
                },
            ),
        })
    }

    fn schedule_slot(
        &self,
        core: &ExecutorCore,
        slot_index: usize,
        generation: u64,
    ) -> Result<(), ExecutorError> {
        self.ready.with(|ready| {
            ready.enqueue(CurrentJob {
                run: run_current_slot,
                core: ::core::ptr::from_ref(core) as usize,
                slot_index,
                generation,
            })
        })??;
        core.request_runtime_dispatch();
        Ok(())
    }

    fn run_next(&self) -> Result<bool, ExecutorError> {
        let job = self.ready.with(CurrentQueueState::dequeue)?;
        if let Some(job) = job {
            unsafe {
                (job.run)(job.core, job.slot_index, job.generation);
            }
            return Ok(true);
        }
        Ok(false)
    }
}

impl ExecutorReactorState {
    fn new(
        capacity: usize,
        fast: bool,
        allocator: &ExecutorDomainAllocator,
    ) -> Result<(Self, CurrentQueue), ExecutorError> {
        let arena_capacity = executor_reactor_capacity(capacity)?;
        let arena = allocator.arena(arena_capacity, executor_reactor_align())?;
        let current_queue = CurrentQueue::new_in(&arena, capacity, fast)?;
        let waits = arena
            .alloc_array_with(capacity, |_| AsyncReactorWaitEntry::EMPTY)
            .map_err(executor_error_from_alloc)?;
        let outcomes = arena
            .alloc_array_with(capacity, |_| None)
            .map_err(executor_error_from_alloc)?;
        #[cfg(feature = "std")]
        let pending_deregister = arena
            .alloc_array_with(capacity, |_| None)
            .map_err(executor_error_from_alloc)?;

        Ok((
            Self {
                poller: ExecutorCell::new(fast, None),
                events: ExecutorCell::new(fast, [EMPTY_EVENT_RECORD; REACTOR_EVENT_BATCH]),
                waits: ExecutorCell::new(fast, waits),
                outcomes: ExecutorCell::new(fast, outcomes),
                #[cfg(feature = "std")]
                pending_deregister: ExecutorCell::new(fast, pending_deregister),
                #[cfg(feature = "std")]
                wake: ExecutorCell::new(fast, None),
            },
            current_queue,
        ))
    }

    #[cfg(feature = "std")]
    fn install_driver_wake_signal(&self) -> Result<(), ExecutorError> {
        let host = system_fiber_host();
        if self.wake.with_ref(Option::is_some)? {
            return Ok(());
        }
        let signal = host
            .create_wake_signal()
            .map_err(executor_error_from_fiber_host)?;
        self.wake.with(|wake| {
            if wake.is_none() {
                *wake = Some(ExecutorReactorWakeSignal { signal, key: None });
            }
        })?;
        Ok(())
    }

    #[cfg(feature = "std")]
    fn signal_driver(&self) -> Result<(), ExecutorError> {
        let Some(()) = self.wake.with_ref(|wake| wake.as_ref().map(|_| ()))? else {
            return Ok(());
        };
        self.wake.with_ref(|wake| {
            if let Some(wake) = wake.as_ref() {
                wake.signal.signal().map_err(executor_error_from_fiber_host)
            } else {
                Ok(())
            }
        })??;
        Ok(())
    }

    fn ensure_poller(&self, reactor: Reactor) -> Result<bool, ExecutorError> {
        self.poller.with(|poller_slot| {
            if poller_slot.is_some() {
                return Ok(true);
            }
            match reactor.create() {
                Ok(poller) => {
                    *poller_slot = Some(poller);
                    Ok(true)
                }
                Err(error) if error.kind() == EventErrorKind::Unsupported => Ok(false),
                Err(error) => Err(executor_error_from_event(error)),
            }
        })?
    }

    #[cfg(feature = "std")]
    fn ensure_wake_registration(&self, reactor: Reactor) -> Result<bool, ExecutorError> {
        if !self.ensure_poller(reactor)? {
            return Ok(false);
        }
        let Some(()) = self.wake.with_ref(|wake| wake.as_ref().map(|_| ()))? else {
            return Ok(true);
        };
        let already_registered = self
            .wake
            .with_ref(|wake| wake.as_ref().and_then(|wake| wake.key).is_some())?;
        if already_registered {
            return Ok(true);
        }

        let source = self.wake.with_ref(|wake| {
            wake.as_ref()
                .ok_or(ExecutorError::Stopped)?
                .signal
                .source_handle()
                .map(EventSourceHandle)
                .map_err(executor_error_from_fiber_host)
        })??;
        let key = self.poller.with(|poller_slot| {
            let poller = poller_slot.as_mut().ok_or(ExecutorError::Stopped)?;
            reactor
                .register(
                    poller,
                    source,
                    EventInterest::READABLE | EventInterest::ERROR | EventInterest::HANGUP,
                )
                .map_err(executor_error_from_event)
        })??;
        self.wake.with(|wake| {
            if let Some(wake) = wake.as_mut() {
                wake.key = Some(key);
            }
        })?;
        Ok(true)
    }

    fn register_readiness_wait(
        &self,
        reactor: Reactor,
        slot_index: usize,
        generation: u64,
        source: EventSourceHandle,
        interest: EventInterest,
    ) -> Result<(), ExecutorError> {
        #[cfg(feature = "std")]
        self.ensure_wake_registration(reactor)?;
        #[cfg(not(feature = "std"))]
        if !self.ensure_poller(reactor)? {
            return Err(ExecutorError::Unsupported);
        }

        let key = self.poller.with(|poller_slot| {
            let poller = poller_slot.as_mut().ok_or(ExecutorError::Unsupported)?;
            reactor
                .register(
                    poller,
                    source,
                    interest | EventInterest::ERROR | EventInterest::HANGUP,
                )
                .map_err(executor_error_from_event)
        })??;
        self.waits.with(|waits| {
            waits[slot_index] = AsyncReactorWaitEntry::readiness(generation, key);
        })?;
        self.outcomes.with(|outcomes| outcomes[slot_index] = None)?;
        #[cfg(feature = "std")]
        self.signal_driver()?;
        Ok(())
    }

    #[cfg(feature = "std")]
    fn queue_readiness_wait(
        &self,
        slot_index: usize,
        generation: u64,
        source: EventSourceHandle,
        interest: EventInterest,
    ) -> Result<(), ExecutorError> {
        self.waits.with(|waits| {
            waits[slot_index] =
                AsyncReactorWaitEntry::readiness_pending(generation, source, interest);
        })?;
        self.outcomes.with(|outcomes| outcomes[slot_index] = None)?;
        self.signal_driver()?;
        Ok(())
    }

    fn register_sleep_wait(
        &self,
        slot_index: usize,
        generation: u64,
        deadline: CanonicalInstant,
    ) -> Result<(), ExecutorError> {
        let raw_deadline = system_monotonic_time()
            .raw_deadline_for_sleep(deadline)
            .map_err(executor_error_from_thread)?;
        self.waits.with(|waits| {
            waits[slot_index] = AsyncReactorWaitEntry::sleep(generation, deadline, raw_deadline);
        })?;
        self.outcomes.with(|outcomes| outcomes[slot_index] = None)?;
        #[cfg(feature = "std")]
        self.signal_driver()?;
        Ok(())
    }

    fn clear_wait(
        &self,
        reactor: Reactor,
        slot_index: usize,
        generation: u64,
    ) -> Result<(), ExecutorError> {
        let removed = self.waits.with(|waits| {
            let entry = waits[slot_index];
            match entry.kind {
                AsyncReactorWaitKind::ReadinessRegistered {
                    generation: live_generation,
                    key,
                } if live_generation == generation => {
                    waits[slot_index] = AsyncReactorWaitEntry::EMPTY;
                    Some(key)
                }
                #[cfg(feature = "std")]
                AsyncReactorWaitKind::ReadinessPending {
                    generation: live_generation,
                    ..
                } if live_generation == generation => {
                    waits[slot_index] = AsyncReactorWaitEntry::EMPTY;
                    None
                }
                AsyncReactorWaitKind::Sleep {
                    generation: live_generation,
                    ..
                } if live_generation == generation => {
                    waits[slot_index] = AsyncReactorWaitEntry::EMPTY;
                    None
                }
                _ => None,
            }
        })?;
        if let Some(key) = removed {
            self.best_effort_deregister(reactor, key)?;
        }
        self.outcomes.with(|outcomes| outcomes[slot_index] = None)?;
        #[cfg(feature = "std")]
        self.signal_driver()?;
        Ok(())
    }

    #[cfg(feature = "std")]
    fn clear_wait_deferred(&self, slot_index: usize, generation: u64) -> Result<(), ExecutorError> {
        let removed = self.waits.with(|waits| {
            let entry = waits[slot_index];
            match entry.kind {
                AsyncReactorWaitKind::ReadinessPending {
                    generation: live_generation,
                    ..
                } if live_generation == generation => {
                    waits[slot_index] = AsyncReactorWaitEntry::EMPTY;
                    None
                }
                AsyncReactorWaitKind::ReadinessRegistered {
                    generation: live_generation,
                    key,
                } if live_generation == generation => {
                    waits[slot_index] = AsyncReactorWaitEntry::EMPTY;
                    Some(key)
                }
                AsyncReactorWaitKind::Sleep {
                    generation: live_generation,
                    ..
                } if live_generation == generation => {
                    waits[slot_index] = AsyncReactorWaitEntry::EMPTY;
                    None
                }
                AsyncReactorWaitKind::None => None,
                _ => None,
            }
        })?;
        if let Some(key) = removed {
            self.pending_deregister.with(|pending| {
                let Some(entry) = pending.iter_mut().find(|entry| entry.is_none()) else {
                    return Err(executor_overflow());
                };
                *entry = Some(key);
                Ok::<(), ExecutorError>(())
            })??;
        }
        self.outcomes.with(|outcomes| outcomes[slot_index] = None)?;
        self.signal_driver()?;
        Ok(())
    }

    fn store_wait_outcome(
        &self,
        slot_index: usize,
        outcome: AsyncWaitOutcome,
    ) -> Result<(), ExecutorError> {
        self.outcomes
            .with(|outcomes| outcomes[slot_index] = Some(outcome))
    }

    fn take_wait_outcome(
        &self,
        slot_index: usize,
    ) -> Result<Option<AsyncWaitOutcome>, ExecutorError> {
        self.outcomes.with(|outcomes| outcomes[slot_index].take())
    }

    fn next_timer_deadline(&self) -> Result<Option<CanonicalInstant>, ExecutorError> {
        self.waits.with_ref(|waits| {
            waits.iter().fold(
                None::<CanonicalInstant>,
                |next_deadline, entry| match entry.kind {
                    AsyncReactorWaitKind::Sleep { deadline, .. } => Some(match next_deadline {
                        Some(current) => current.min(deadline),
                        None => deadline,
                    }),
                    _ => next_deadline,
                },
            )
        })
    }

    fn has_readiness_waiters(&self) -> Result<bool, ExecutorError> {
        self.waits.with_ref(|waits| {
            waits.iter().any(|entry| match entry.kind {
                #[cfg(feature = "std")]
                AsyncReactorWaitKind::ReadinessPending { .. } => true,
                AsyncReactorWaitKind::ReadinessRegistered { .. } => true,
                AsyncReactorWaitKind::None | AsyncReactorWaitKind::Sleep { .. } => false,
            })
        })
    }

    #[cfg(feature = "std")]
    fn flush_pending_deregistrations(&self, reactor: Reactor) -> Result<(), ExecutorError> {
        loop {
            let key = self.pending_deregister.with(|queue| {
                Ok::<Option<EventKey>, ExecutorError>(
                    queue.iter_mut().find_map(|entry| entry.take()),
                )
            })??;
            let Some(key) = key else {
                break;
            };
            self.best_effort_deregister(reactor, key)?;
        }
        Ok(())
    }

    #[cfg(feature = "std")]
    fn activate_pending_readiness_waits(
        &self,
        core: &ExecutorCore,
        reactor: Reactor,
    ) -> Result<bool, ExecutorError> {
        let mut progressed = false;
        let slot_count = self.waits.with_ref(|waits| waits.len())?;
        for slot_index in 0..slot_count {
            let pending = self.waits.with_ref(|waits| match waits[slot_index].kind {
                AsyncReactorWaitKind::ReadinessPending {
                    generation,
                    source,
                    interest,
                } => Some((generation, source, interest)),
                _ => None,
            })?;
            let Some((generation, source, interest)) = pending else {
                continue;
            };

            let key = match self.poller.with(|poller_slot| {
                let poller = poller_slot.as_mut().ok_or(ExecutorError::Unsupported)?;
                reactor
                    .register(
                        poller,
                        source,
                        interest | EventInterest::ERROR | EventInterest::HANGUP,
                    )
                    .map_err(executor_error_from_event)
            })? {
                Ok(key) => key,
                Err(error) => {
                    self.waits.with(|waits| {
                        if matches!(
                            waits[slot_index].kind,
                            AsyncReactorWaitKind::ReadinessPending {
                                generation: live_generation,
                                ..
                            } if live_generation == generation
                        ) {
                            waits[slot_index] = AsyncReactorWaitEntry::EMPTY;
                        }
                    })?;
                    self.store_wait_outcome(slot_index, AsyncWaitOutcome::Error(error))?;
                    core.schedule_slot(slot_index, generation)?;
                    progressed = true;
                    continue;
                }
            };

            self.waits.with(|waits| {
                if matches!(
                    waits[slot_index].kind,
                    AsyncReactorWaitKind::ReadinessPending {
                        generation: live_generation,
                        ..
                    } if live_generation == generation
                ) {
                    waits[slot_index] = AsyncReactorWaitEntry::readiness(generation, key);
                }
            })?;
        }
        Ok(progressed)
    }

    fn collect_due_timers(
        &self,
        core: &ExecutorCore,
        now: CanonicalInstant,
        now_raw: Option<MonotonicRawInstant>,
    ) -> Result<bool, ExecutorError> {
        let mut progressed = false;
        let slot_count = self.waits.with_ref(|waits| waits.len())?;
        for slot_index in 0..slot_count {
            let generation = self.waits.with(|waits| {
                let AsyncReactorWaitKind::Sleep {
                    generation,
                    deadline,
                    raw_deadline,
                } = waits[slot_index].kind
                else {
                    return Ok::<Option<u64>, ExecutorError>(None);
                };
                let due = match (now_raw, raw_deadline) {
                    (Some(now_raw), Some(raw_deadline)) => now_raw.deadline_reached(raw_deadline),
                    _ => now >= deadline,
                };
                if !due {
                    return Ok(None);
                }
                waits[slot_index] = AsyncReactorWaitEntry::EMPTY;
                Ok(Some(generation))
            })??;
            let Some(generation) = generation else {
                continue;
            };
            self.store_wait_outcome(slot_index, AsyncWaitOutcome::Timer)?;
            core.schedule_slot(slot_index, generation)?;
            progressed = true;
        }
        Ok(progressed)
    }

    fn resolve_reactor_events(
        &self,
        core: &ExecutorCore,
        reactor: Reactor,
        count: usize,
    ) -> Result<bool, ExecutorError> {
        if count == 0 {
            return Ok(false);
        }
        #[cfg(feature = "std")]
        let mut wake_event = false;
        #[cfg(feature = "std")]
        let wake_key = self
            .wake
            .with_ref(|wake| wake.as_ref().and_then(|wake| wake.key))?;

        let mut progressed = false;
        for event_index in 0..count {
            let event = self.events.with_ref(|events| events[event_index])?;
            #[cfg(feature = "std")]
            if Some(event.key) == wake_key {
                wake_event = true;
                continue;
            }
            let EventNotification::Readiness(readiness) = event.notification else {
                continue;
            };
            let ready = self.waits.with(|waits| {
                for (slot_index, entry) in waits.iter_mut().enumerate() {
                    let AsyncReactorWaitKind::ReadinessRegistered { generation, key } = entry.kind
                    else {
                        continue;
                    };
                    if key != event.key {
                        continue;
                    }
                    entry.kind = AsyncReactorWaitKind::None;
                    return Ok::<Option<(usize, u64)>, ExecutorError>(Some((
                        slot_index, generation,
                    )));
                }
                Ok(None)
            })??;
            let Some((slot_index, generation)) = ready else {
                continue;
            };
            self.best_effort_deregister(reactor, event.key)?;
            self.store_wait_outcome(slot_index, AsyncWaitOutcome::Readiness(readiness))?;
            core.schedule_slot(slot_index, generation)?;
            progressed = true;
        }

        #[cfg(feature = "std")]
        if wake_event {
            self.wake.with_ref(|wake| {
                if let Some(wake) = wake.as_ref() {
                    wake.signal.drain().map_err(executor_error_from_fiber_host)
                } else {
                    Ok(())
                }
            })??;
        }
        Ok(progressed)
    }

    fn best_effort_deregister(&self, reactor: Reactor, key: EventKey) -> Result<(), ExecutorError> {
        if !self.ensure_poller(reactor)? {
            return Ok(());
        }
        let result = self.poller.with(|poller_slot| {
            let Some(poller) = poller_slot.as_mut() else {
                return Ok(());
            };
            match reactor.deregister(poller, key) {
                Ok(()) => Ok(()),
                Err(error)
                    if matches!(
                        error.kind(),
                        EventErrorKind::Invalid | EventErrorKind::StateConflict
                    ) =>
                {
                    Ok(())
                }
                Err(error) => Err(executor_error_from_event(error)),
            }
        })?;
        result
    }

    fn drive(
        &self,
        core: &ExecutorCore,
        blocking: bool,
        max_events: Option<usize>,
    ) -> Result<bool, ExecutorError> {
        let mut progressed = false;
        #[cfg(feature = "std")]
        if blocking {
            self.ensure_wake_registration(core.reactor)?;
        }
        if !self.ensure_poller(core.reactor)? {
            return Ok(progressed);
        }
        #[cfg(feature = "std")]
        {
            self.flush_pending_deregistrations(core.reactor)?;
            progressed |= self.activate_pending_readiness_waits(core, core.reactor)?;
        }
        let now = if self.next_timer_deadline()?.is_some() {
            Some(runtime_monotonic_now_instant()?)
        } else {
            None
        };
        if let Some(now) = now {
            let now_raw = runtime_monotonic_raw_now().ok();
            progressed |= self.collect_due_timers(core, now, now_raw)?;
        }

        let has_readiness_waiters = self.has_readiness_waiters()?;
        let next_deadline = self.next_timer_deadline()?;
        let should_poll = has_readiness_waiters || (blocking && next_deadline.is_some());
        if !should_poll {
            return Ok(progressed);
        }

        if blocking
            && !has_readiness_waiters
            && let Some(deadline) = next_deadline
        {
            system_monotonic_time()
                .sleep_until(deadline)
                .map_err(executor_error_from_thread)?;
            let now = runtime_monotonic_now_instant()?;
            let now_raw = runtime_monotonic_raw_now().ok();
            progressed |= self.collect_due_timers(core, now, now_raw)?;
            return Ok(progressed);
        }

        let timeout = if blocking {
            match next_deadline {
                Some(deadline) => Some(runtime_monotonic_duration_until(deadline)?),
                None => None,
            }
        } else {
            Some(Duration::from_millis(0))
        };

        let count = self.poller.with(|poller_slot| {
            let Some(poller) = poller_slot.as_mut() else {
                return Ok(0);
            };
            self.events.with(|events| {
                let limit = max_events.unwrap_or(events.len()).min(events.len());
                core.reactor
                    .poll(poller, &mut events[..limit], timeout)
                    .map_err(executor_error_from_event)
            })?
        })??;
        progressed |= self.resolve_reactor_events(core, core.reactor, count)?;

        if self.next_timer_deadline()?.is_some() {
            let now = runtime_monotonic_now_instant()?;
            let now_raw = runtime_monotonic_raw_now().ok();
            progressed |= self.collect_due_timers(core, now, now_raw)?;
        }
        Ok(progressed)
    }
}

impl fmt::Debug for CurrentQueue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CurrentQueue").finish_non_exhaustive()
    }
}

impl CurrentQueueState {
    fn enqueue(&mut self, job: CurrentJob) -> Result<(), ExecutorError> {
        if self.len == self.entries.len() {
            return Err(executor_overflow());
        }
        self.entries[self.tail] = Some(job);
        self.tail = (self.tail + 1) % self.entries.len();
        self.len += 1;
        Ok(())
    }

    fn dequeue(&mut self) -> Option<CurrentJob> {
        if self.len == 0 {
            return None;
        }
        let job = self.entries[self.head].take();
        self.head = (self.head + 1) % self.entries.len();
        self.len -= 1;
        job
    }
}

#[cfg(feature = "std")]
impl HostedReadyQueueState {
    const fn new() -> Self {
        Self {
            entries: [None; CURRENT_QUEUE_CAPACITY],
            head: 0,
            tail: 0,
            len: 0,
        }
    }

    fn enqueue(&mut self, job: CurrentJob) -> Result<(), ExecutorError> {
        if self.len == self.entries.len() {
            return Err(executor_overflow());
        }
        self.entries[self.tail] = Some(job);
        self.tail = (self.tail + 1) % self.entries.len();
        self.len += 1;
        Ok(())
    }

    fn dequeue(&mut self) -> Option<CurrentJob> {
        if self.len == 0 {
            return None;
        }
        let job = self.entries[self.head].take();
        self.head = (self.head + 1) % self.entries.len();
        self.len -= 1;
        job
    }

    #[allow(dead_code)]
    fn clear(&mut self) -> usize {
        let dropped = self.len;
        while self.dequeue().is_some() {}
        dropped
    }
}

#[derive(Debug)]
struct FixedIndexStack {
    entries: ArenaSlice<usize>,
    len: usize,
}

impl FixedIndexStack {
    fn new_in(arena: &BoundedArena, capacity: usize) -> Result<Self, ExecutorError> {
        let entries = arena
            .alloc_array_with(capacity, |index| capacity.saturating_sub(index + 1))
            .map_err(executor_error_from_alloc)?;
        let len = entries.len();
        Ok(Self { entries, len })
    }

    fn pop(&mut self) -> Option<usize> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        Some(self.entries[self.len])
    }

    fn push(&mut self, value: usize) -> Result<(), ExecutorError> {
        if self.len == self.entries.len() {
            return Err(executor_invalid());
        }
        self.entries[self.len] = value;
        self.len += 1;
        Ok(())
    }

    fn contains(&self, value: usize) -> bool {
        self.entries[..self.len].iter().any(|entry| *entry == value)
    }
}

type InlineAsyncPollFn = unsafe fn(
    &mut InlineAsyncFutureStorage,
    &ExecutorCell<InlineAsyncResultStorage>,
    &AsyncTaskSpillStore,
    &mut Context<'_>,
) -> Result<Poll<()>, ExecutorError>;

struct InlineAsyncFutureStorage {
    allocation: Option<ExtentLease>,
    poll: Option<InlineAsyncPollFn>,
    drop: Option<unsafe fn(*mut u8)>,
    occupied: bool,
}

impl fmt::Debug for InlineAsyncFutureStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InlineAsyncFutureStorage")
            .field("occupied", &self.occupied)
            .finish_non_exhaustive()
    }
}

impl InlineAsyncFutureStorage {
    const fn empty() -> Self {
        Self {
            allocation: None,
            poll: None,
            drop: None,
            occupied: false,
        }
    }

    fn store_future<F>(
        &mut self,
        spill_store: &AsyncTaskSpillStore,
        future: F,
    ) -> Result<(), ExecutorError>
    where
        F: Future + 'static,
        F::Output: 'static,
    {
        if self.occupied {
            return Err(executor_invalid());
        }
        self.allocation = Some(spill_store.allocate_task_envelope::<F>()?);
        let target = self
            .allocation
            .as_ref()
            .ok_or_else(executor_invalid)?
            .as_non_null()
            .as_ptr()
            .cast::<F>();
        unsafe {
            target.write(future);
        }
        self.poll = Some(poll_inline_async_future::<F>);
        self.drop = Some(drop_inline_async_value::<F>);
        self.occupied = true;
        Ok(())
    }

    fn poll_in_place(
        &mut self,
        result: &ExecutorCell<InlineAsyncResultStorage>,
        spill_store: &AsyncTaskSpillStore,
        context: &mut Context<'_>,
    ) -> Result<Poll<()>, ExecutorError> {
        if !self.occupied {
            return Err(executor_invalid());
        }
        let poll = self.poll.ok_or_else(executor_invalid)?;
        unsafe { poll(self, result, spill_store, context) }
    }

    fn clear(&mut self, spill_store: &AsyncTaskSpillStore) -> Result<(), ExecutorError> {
        self.drop_value_only();
        if let Some(allocation) = self.allocation.take() {
            spill_store.deallocate(allocation)?;
        }
        self.poll = None;
        Ok(())
    }

    fn storage_ptr(&mut self) -> *mut u8 {
        self.allocation
            .as_ref()
            .expect("async futures always live inside one exact lifecycle envelope")
            .as_non_null()
            .as_ptr()
    }

    fn take_allocation(&mut self) -> Option<ExtentLease> {
        self.allocation.take()
    }

    fn drop_value_only(&mut self) {
        if !self.occupied {
            self.poll = None;
            self.drop = None;
            return;
        }

        if let Some(drop) = self.drop.take() {
            unsafe {
                drop(self.storage_ptr());
            }
        }
        self.poll = None;
        self.occupied = false;
    }
}

impl Drop for InlineAsyncFutureStorage {
    fn drop(&mut self) {
        self.drop_value_only();
    }
}

#[derive(Debug)]
struct AsyncTaskSpillStore {
    allocator: Option<ExecutorDomainAllocator>,
}

impl AsyncTaskSpillStore {
    fn new(_fast: bool, allocator: Option<ExecutorDomainAllocator>) -> Self {
        Self { allocator }
    }

    fn supports_layout(&self, _len: usize, _align: usize) -> bool {
        self.allocator.is_some()
    }

    fn allocate_for_layout(&self, len: usize, align: usize) -> Result<ExtentLease, ExecutorError> {
        let len = executor_exact_backing_len(len);
        if !self.supports_layout(len, align) {
            return Err(ExecutorError::Unsupported);
        }
        self.allocator
            .as_ref()
            .ok_or(ExecutorError::Unsupported)?
            .extent(MemoryPoolExtentRequest { len, align })
    }

    fn allocate_for<T: 'static>(&self) -> Result<ExtentLease, ExecutorError> {
        self.allocate_for_layout(size_of::<T>(), align_of::<T>())
    }

    fn allocate_task_envelope<F>(&self) -> Result<ExtentLease, ExecutorError>
    where
        F: Future + 'static,
        F::Output: 'static,
    {
        let len = executor_exact_backing_len(size_of::<F>().max(size_of::<F::Output>()));
        let align = align_of::<F>().max(align_of::<F::Output>());
        self.allocate_for_layout(len, align)
    }

    fn deallocate(&self, allocation: ExtentLease) -> Result<(), ExecutorError> {
        drop(allocation);
        Ok(())
    }
}

struct InlineAsyncResultStorage {
    allocation: Option<ExtentLease>,
    drop: Option<unsafe fn(*mut u8)>,
    type_id: Option<TypeId>,
    occupied: bool,
}

impl fmt::Debug for InlineAsyncResultStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InlineAsyncResultStorage")
            .field("occupied", &self.occupied)
            .finish_non_exhaustive()
    }
}

impl InlineAsyncResultStorage {
    const fn empty() -> Self {
        Self {
            allocation: None,
            drop: None,
            type_id: None,
            occupied: false,
        }
    }

    fn store_with_allocation<T: 'static>(
        &mut self,
        spill_store: &AsyncTaskSpillStore,
        carried_allocation: Option<ExtentLease>,
        value: T,
    ) -> Result<(), ExecutorError> {
        if self.occupied {
            return Err(executor_invalid());
        }
        let allocation = match carried_allocation {
            Some(allocation) => allocation,
            None => spill_store.allocate_for::<T>()?,
        };
        let target = allocation.as_non_null().as_ptr().cast::<T>();
        self.allocation = Some(allocation);
        unsafe {
            target.write(value);
        }
        self.drop = Some(drop_inline_async_value::<T>);
        self.type_id = Some(TypeId::of::<T>());
        self.occupied = true;
        Ok(())
    }

    fn take<T: 'static>(&mut self, spill_store: &AsyncTaskSpillStore) -> Result<T, ExecutorError> {
        if !self.occupied || self.type_id != Some(TypeId::of::<T>()) {
            return Err(executor_invalid());
        }

        self.drop = None;
        self.type_id = None;
        self.occupied = false;
        let value = unsafe { self.storage_ptr().cast::<T>().read() };
        if let Some(allocation) = self.allocation.take() {
            spill_store.deallocate(allocation)?;
        }
        Ok(value)
    }

    fn clear(&mut self, spill_store: &AsyncTaskSpillStore) -> Result<(), ExecutorError> {
        self.drop_value_only();
        if let Some(allocation) = self.allocation.take() {
            spill_store.deallocate(allocation)?;
        }
        self.type_id = None;
        Ok(())
    }

    fn storage_ptr(&mut self) -> *mut u8 {
        self.allocation
            .as_ref()
            .expect("async results always live inside one exact lifecycle envelope")
            .as_non_null()
            .as_ptr()
    }

    fn drop_value_only(&mut self) {
        if !self.occupied {
            self.drop = None;
            self.type_id = None;
            return;
        }

        if let Some(drop) = self.drop.take() {
            unsafe {
                drop(self.storage_ptr());
            }
        }
        self.type_id = None;
        self.occupied = false;
    }
}

impl Drop for InlineAsyncResultStorage {
    fn drop(&mut self) {
        self.drop_value_only();
    }
}

unsafe fn poll_inline_async_future<F>(
    future_storage: &mut InlineAsyncFutureStorage,
    result: &ExecutorCell<InlineAsyncResultStorage>,
    spill_store: &AsyncTaskSpillStore,
    context: &mut Context<'_>,
) -> Result<Poll<()>, ExecutorError>
where
    F: Future + 'static,
    F::Output: 'static,
{
    // SAFETY: executor futures live inside arena-backed task slots whose addresses remain stable
    // for the lifetime of the live slot lease; the arena never relocates allocations.
    let future = unsafe { Pin::new_unchecked(&mut *future_storage.storage_ptr().cast::<F>()) };

    #[cfg(feature = "std")]
    match poll_future_contained(future, context) {
        Ok(Poll::Ready(output)) => {
            future_storage.drop_value_only();
            let allocation = future_storage.take_allocation();
            result
                .with(|result| result.store_with_allocation(spill_store, allocation, output))??;
            Ok(Poll::Ready(()))
        }
        Ok(Poll::Pending) => Ok(Poll::Pending),
        Err(()) => Err(ExecutorError::TaskPanicked),
    }

    #[cfg(not(feature = "std"))]
    match poll_future_contained(future, context) {
        Poll::Ready(output) => {
            future_storage.drop_value_only();
            let allocation = future_storage.take_allocation();
            result
                .with(|result| result.store_with_allocation(spill_store, allocation, output))??;
            Ok(Poll::Ready(()))
        }
        Poll::Pending => Ok(Poll::Pending),
    }
}

unsafe fn drop_inline_async_value<T>(ptr: *mut u8) {
    unsafe {
        ptr.cast::<T>().drop_in_place();
    }
}

#[derive(Debug)]
struct AsyncTaskWakerData {
    core_ptr: AtomicUsize,
    slot_index: usize,
    generation: AtomicUsize,
}

impl AsyncTaskWakerData {
    const fn new(slot_index: usize) -> Self {
        Self {
            core_ptr: AtomicUsize::new(0),
            slot_index,
            generation: AtomicUsize::new(0),
        }
    }

    fn set_core(&self, core: *const ExecutorCore) {
        self.core_ptr.store(core as usize, Ordering::Release);
    }

    fn core_ptr(&self) -> *const ExecutorCore {
        let core_ptr = self.core_ptr.load(Ordering::Acquire);
        if core_ptr == 0 {
            return ::core::ptr::null();
        }
        core_ptr as *const ExecutorCore
    }

    fn set_generation(&self, generation: u64) {
        self.generation.store(
            usize::try_from(generation).unwrap_or(usize::MAX),
            Ordering::Release,
        );
    }

    fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire) as u64
    }
}
