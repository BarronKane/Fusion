use core::cell::UnsafeCell;
use core::hint::spin_loop;
use core::mem::MaybeUninit;
use core::sync::atomic::{
    AtomicBool,
    AtomicU8,
    Ordering,
};

use crate::contract::pal::vector::{
    VectorBaseContract,
    VectorCaps,
    VectorError,
    VectorOwnershipControlContract,
    VectorOwnershipKind,
    VectorTableMode,
    VectorTableTopology,
};

use super::platform::vector::{
    PlatformVectorBuilder,
    bind_reserved_event_timeout_wake,
    system_vector,
};

const RUNTIME_VECTOR_UNINITIALIZED: u8 = 0;
const RUNTIME_VECTOR_RUNNING: u8 = 1;
const RUNTIME_VECTOR_READY: u8 = 2;
const RUNTIME_VECTOR_SKIPPED: u8 = 3;

struct RuntimeVectorBroker {
    state: AtomicU8,
    lock: AtomicBool,
    builder: UnsafeCell<MaybeUninit<PlatformVectorBuilder>>,
}

impl RuntimeVectorBroker {
    const fn new() -> Self {
        Self {
            state: AtomicU8::new(RUNTIME_VECTOR_UNINITIALIZED),
            lock: AtomicBool::new(false),
            builder: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    fn ensure(&self) -> Result<(), VectorError> {
        loop {
            match self.state.load(Ordering::Acquire) {
                RUNTIME_VECTOR_READY => return Ok(()),
                RUNTIME_VECTOR_SKIPPED => return Err(VectorError::unsupported()),
                RUNTIME_VECTOR_RUNNING => spin_loop(),
                RUNTIME_VECTOR_UNINITIALIZED => {
                    if self
                        .state
                        .compare_exchange(
                            RUNTIME_VECTOR_UNINITIALIZED,
                            RUNTIME_VECTOR_RUNNING,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_err()
                    {
                        continue;
                    }

                    match bootstrap_runtime_vector_builder() {
                        Ok(Some(builder)) => {
                            // SAFETY: successful initialization happens once and the builder stays
                            // pinned in place for process lifetime behind the broker lock.
                            unsafe { (*self.builder.get()).write(builder) };
                            self.state.store(RUNTIME_VECTOR_READY, Ordering::Release);
                            return Ok(());
                        }
                        Ok(None) => {
                            self.state.store(RUNTIME_VECTOR_SKIPPED, Ordering::Release);
                            return Err(VectorError::unsupported());
                        }
                        Err(error) => {
                            self.state
                                .store(RUNTIME_VECTOR_UNINITIALIZED, Ordering::Release);
                            return Err(error);
                        }
                    }
                }
                _ => return Err(VectorError::state_conflict()),
            }
        }
    }

    fn with_builder<R>(
        &self,
        bind: impl FnOnce(&mut PlatformVectorBuilder) -> R,
    ) -> Result<R, VectorError> {
        self.ensure()?;
        let _guard = RuntimeVectorBrokerGuard::acquire(&self.lock)?;
        match self.state.load(Ordering::Acquire) {
            RUNTIME_VECTOR_READY => {
                // SAFETY: initialization is one-time, the builder is never moved again, and the
                // broker lock serializes mutable access.
                let builder = unsafe { (*self.builder.get()).assume_init_mut() };
                Ok(bind(builder))
            }
            RUNTIME_VECTOR_SKIPPED => Err(VectorError::unsupported()),
            _ => Err(VectorError::state_conflict()),
        }
    }
}

unsafe impl Sync for RuntimeVectorBroker {}

struct RuntimeVectorBrokerGuard<'a> {
    lock: &'a AtomicBool,
}

impl<'a> RuntimeVectorBrokerGuard<'a> {
    fn acquire(lock: &'a AtomicBool) -> Result<Self, VectorError> {
        loop {
            match lock.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => return Ok(Self { lock }),
                Err(true) => spin_loop(),
                Err(false) => continue,
            }
        }
    }
}

impl Drop for RuntimeVectorBrokerGuard<'_> {
    fn drop(&mut self) {
        self.lock.store(false, Ordering::Release);
    }
}

static RUNTIME_VECTOR_BROKER: RuntimeVectorBroker = RuntimeVectorBroker::new();

pub fn ensure_runtime_reserved_wake_vectors() -> Result<(), VectorError> {
    RUNTIME_VECTOR_BROKER.ensure()
}

pub fn ensure_runtime_reserved_wake_vectors_best_effort() {
    let _ = ensure_runtime_reserved_wake_vectors();
}

pub fn with_runtime_vector_builder<R>(
    bind: impl FnOnce(&mut PlatformVectorBuilder) -> R,
) -> Result<R, VectorError> {
    RUNTIME_VECTOR_BROKER.with_builder(bind)
}

fn bootstrap_runtime_vector_builder() -> Result<Option<PlatformVectorBuilder>, VectorError> {
    let vector = system_vector();
    let support = VectorBaseContract::support(&vector);
    if !support
        .caps
        .contains(VectorCaps::ADOPT_AND_CLONE | VectorCaps::INLINE_DISPATCH)
    {
        return Ok(None);
    }

    let mode = VectorBaseContract::table_mode(&vector);
    if mode.ownership != VectorOwnershipKind::Unowned {
        return Err(VectorError::state_conflict());
    }

    let mut builder = VectorOwnershipControlContract::adopt_and_clone(
        &vector,
        VectorTableMode {
            ownership: VectorOwnershipKind::AdoptedOwned,
            topology: VectorTableTopology::SharedTable,
            domain: mode.domain,
        },
    )?;

    match bind_reserved_event_timeout_wake(&mut builder, None) {
        Ok(()) => Ok(Some(builder)),
        Err(error)
            if error.kind() == crate::contract::pal::vector::VectorErrorKind::Unsupported =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}
