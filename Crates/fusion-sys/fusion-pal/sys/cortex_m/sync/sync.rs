//! Cortex-M bare-metal synchronization backend.
//!
//! This backend intentionally exposes only local, spin-based primitives. There is no kernel
//! waiter queue hiding behind the curtains, and pretending otherwise would just be a more
//! sophisticated form of lying.

#![allow(clippy::cast_possible_truncation)]

#[cfg(any(
    not(any(target_has_atomic = "8", target_has_atomic = "32")),
    not(any(target_has_atomic = "16", target_has_atomic = "32"))
))]
use core::arch::asm;
#[cfg(any(
    not(any(target_has_atomic = "8", target_has_atomic = "32")),
    not(any(target_has_atomic = "16", target_has_atomic = "32"))
))]
use core::cell::UnsafeCell;
use core::hint::spin_loop;
#[cfg(target_has_atomic = "8")]
use core::sync::atomic::AtomicU8;
#[cfg(target_has_atomic = "16")]
use core::sync::atomic::AtomicU16;
#[cfg(any(
    not(any(target_has_atomic = "8", target_has_atomic = "32")),
    not(any(target_has_atomic = "16", target_has_atomic = "32"))
))]
use core::sync::atomic::compiler_fence;
use core::sync::atomic::{AtomicU32, Ordering};
use core::time::Duration;

use crate::pal::sync::{
    MutexCaps,
    MutexSupport,
    OnceBeginResult,
    OnceCaps,
    OnceState,
    OnceSupport,
    PriorityInheritanceSupport,
    ProcessScopeSupport,
    RawMutex,
    RawOnce,
    RawRwLock,
    RawSemaphore,
    RecursionSupport,
    RobustnessSupport,
    RwLockCaps,
    RwLockFairnessSupport,
    RwLockSupport,
    SemaphoreCaps,
    SemaphoreSupport,
    SyncBase,
    SyncError,
    SyncFallbackKind,
    SyncImplementationKind,
    SyncSupport,
    TimeoutCaps,
    WaitOutcome,
    WaitPrimitive,
    WaitSupport,
};

const MUTEX_UNLOCKED: u8 = 0;
const MUTEX_LOCKED: u8 = 1;
#[cfg(all(not(target_has_atomic = "8"), target_has_atomic = "32"))]
const MUTEX_UNLOCKED_WORD: u32 = 0;
#[cfg(all(not(target_has_atomic = "8"), target_has_atomic = "32"))]
const MUTEX_LOCKED_WORD: u32 = 1;

const ONCE_UNINITIALIZED: u8 = 0;
const ONCE_RUNNING: u8 = 1;
const ONCE_COMPLETE: u8 = 2;
#[cfg(all(not(target_has_atomic = "8"), target_has_atomic = "32"))]
const ONCE_UNINITIALIZED_WORD: u32 = 0;
#[cfg(all(not(target_has_atomic = "8"), target_has_atomic = "32"))]
const ONCE_RUNNING_WORD: u32 = 1;
#[cfg(all(not(target_has_atomic = "8"), target_has_atomic = "32"))]
const ONCE_COMPLETE_WORD: u32 = 2;

#[cfg(target_has_atomic = "16")]
const RWLOCK_WRITER: u16 = 0x8000;
#[cfg(target_has_atomic = "16")]
const RWLOCK_READERS_MASK: u16 = 0x7fff;
#[cfg(not(target_has_atomic = "16"))]
const RWLOCK_WRITER_WORD: u32 = 0x8000_0000;
#[cfg(not(target_has_atomic = "16"))]
const RWLOCK_READERS_MASK_WORD: u32 = 0x7fff_ffff;

const CORTEX_M_LOCAL_CRITICAL_SECTION_SYNC_SAFE: bool =
    super::super::hal::soc::board::LOCAL_CRITICAL_SECTION_SYNC_SAFE;

const CORTEX_M_MUTEX_SUPPORT_ATOMIC: MutexSupport = MutexSupport {
    caps: MutexCaps::TRY_LOCK
        .union(MutexCaps::BLOCKING)
        .union(MutexCaps::STATIC_INIT),
    timeout: TimeoutCaps::empty(),
    priority_inheritance: PriorityInheritanceSupport::None,
    recursion: RecursionSupport::None,
    robustness: RobustnessSupport::None,
    process_scope: ProcessScopeSupport::LocalOnly,
    implementation: SyncImplementationKind::Emulated,
    fallback: SyncFallbackKind::SpinOnly,
};

const CORTEX_M_MUTEX_SUPPORT_CRITICAL: MutexSupport = MutexSupport {
    caps: MutexCaps::TRY_LOCK
        .union(MutexCaps::BLOCKING)
        .union(MutexCaps::STATIC_INIT),
    timeout: TimeoutCaps::empty(),
    priority_inheritance: PriorityInheritanceSupport::None,
    recursion: RecursionSupport::None,
    robustness: RobustnessSupport::None,
    process_scope: ProcessScopeSupport::LocalOnly,
    implementation: SyncImplementationKind::Emulated,
    fallback: SyncFallbackKind::CriticalSection,
};

const CORTEX_M_ONCE_SUPPORT_ATOMIC: OnceSupport = OnceSupport {
    caps: OnceCaps::WAITING
        .union(OnceCaps::STATIC_INIT)
        .union(OnceCaps::RESET_ON_FAILURE),
    process_scope: ProcessScopeSupport::LocalOnly,
    implementation: SyncImplementationKind::Emulated,
    fallback: SyncFallbackKind::SpinOnly,
};

const CORTEX_M_ONCE_SUPPORT_CRITICAL: OnceSupport = OnceSupport {
    caps: OnceCaps::WAITING
        .union(OnceCaps::STATIC_INIT)
        .union(OnceCaps::RESET_ON_FAILURE),
    process_scope: ProcessScopeSupport::LocalOnly,
    implementation: SyncImplementationKind::Emulated,
    fallback: SyncFallbackKind::CriticalSection,
};

const CORTEX_M_SEMAPHORE_SUPPORT_ATOMIC: SemaphoreSupport = SemaphoreSupport {
    caps: SemaphoreCaps::TRY_ACQUIRE
        .union(SemaphoreCaps::BLOCKING)
        .union(SemaphoreCaps::RELEASE_MANY),
    timeout: TimeoutCaps::empty(),
    process_scope: ProcessScopeSupport::LocalOnly,
    implementation: SyncImplementationKind::Emulated,
    fallback: SyncFallbackKind::SpinOnly,
};

const CORTEX_M_SEMAPHORE_SUPPORT_CRITICAL: SemaphoreSupport = SemaphoreSupport {
    caps: SemaphoreCaps::TRY_ACQUIRE
        .union(SemaphoreCaps::BLOCKING)
        .union(SemaphoreCaps::RELEASE_MANY),
    timeout: TimeoutCaps::empty(),
    process_scope: ProcessScopeSupport::LocalOnly,
    implementation: SyncImplementationKind::Emulated,
    fallback: SyncFallbackKind::CriticalSection,
};

const CORTEX_M_RWLOCK_SUPPORT_ATOMIC: RwLockSupport = RwLockSupport {
    caps: RwLockCaps::TRY_READ
        .union(RwLockCaps::TRY_WRITE)
        .union(RwLockCaps::BLOCKING_READ)
        .union(RwLockCaps::BLOCKING_WRITE)
        .union(RwLockCaps::STATIC_INIT),
    timeout: TimeoutCaps::empty(),
    fairness: RwLockFairnessSupport::None,
    process_scope: ProcessScopeSupport::LocalOnly,
    implementation: SyncImplementationKind::Emulated,
    fallback: SyncFallbackKind::SpinOnly,
};

const CORTEX_M_RWLOCK_SUPPORT_CRITICAL: RwLockSupport = RwLockSupport {
    caps: RwLockCaps::TRY_READ
        .union(RwLockCaps::TRY_WRITE)
        .union(RwLockCaps::BLOCKING_READ)
        .union(RwLockCaps::BLOCKING_WRITE)
        .union(RwLockCaps::STATIC_INIT),
    timeout: TimeoutCaps::empty(),
    fairness: RwLockFairnessSupport::None,
    process_scope: ProcessScopeSupport::LocalOnly,
    implementation: SyncImplementationKind::Emulated,
    fallback: SyncFallbackKind::CriticalSection,
};

/// Cortex-M synchronization provider handle.
#[derive(Debug, Clone, Copy, Default)]
pub struct CortexMSync;

/// Cortex-M raw mutex implemented with a local spin word.
#[derive(Debug, Default)]
pub struct CortexMRawMutex {
    #[cfg(target_has_atomic = "8")]
    state: AtomicU8,
    #[cfg(all(not(target_has_atomic = "8"), target_has_atomic = "32"))]
    state: AtomicU32,
    #[cfg(not(any(target_has_atomic = "8", target_has_atomic = "32")))]
    state: UnsafeCell<u8>,
}

/// Cortex-M raw once primitive implemented with a local spin word.
#[derive(Debug, Default)]
pub struct CortexMRawOnce {
    #[cfg(target_has_atomic = "8")]
    state: AtomicU8,
    #[cfg(all(not(target_has_atomic = "8"), target_has_atomic = "32"))]
    state: AtomicU32,
    #[cfg(not(any(target_has_atomic = "8", target_has_atomic = "32")))]
    state: UnsafeCell<u8>,
}

/// Cortex-M counting semaphore implemented with a local spin counter.
#[derive(Debug)]
pub struct CortexMSemaphore {
    #[cfg(target_has_atomic = "16")]
    permits: AtomicU16,
    #[cfg(target_has_atomic = "16")]
    max: u16,
    #[cfg(all(not(target_has_atomic = "16"), target_has_atomic = "32"))]
    permits: AtomicU32,
    #[cfg(all(not(target_has_atomic = "16"), target_has_atomic = "32"))]
    max: u32,
    #[cfg(not(any(target_has_atomic = "16", target_has_atomic = "32")))]
    permits: UnsafeCell<u32>,
    #[cfg(not(any(target_has_atomic = "16", target_has_atomic = "32")))]
    max: u32,
}

/// Cortex-M raw reader/writer lock implemented with a local spin word.
#[derive(Debug, Default)]
pub struct CortexMRawRwLock {
    #[cfg(target_has_atomic = "16")]
    state: AtomicU16,
    #[cfg(all(not(target_has_atomic = "16"), target_has_atomic = "32"))]
    state: AtomicU32,
    #[cfg(not(any(target_has_atomic = "16", target_has_atomic = "32")))]
    state: UnsafeCell<u32>,
}

// SAFETY: these primitives coordinate all interior mutation through either atomics or an
// interrupt-masked local critical section, and return `unsupported` rather than touching the
// fallback state on boards where that local critical-section path is not truthful.
unsafe impl Send for CortexMRawMutex {}
// SAFETY: see above.
unsafe impl Sync for CortexMRawMutex {}
// SAFETY: see above.
unsafe impl Send for CortexMRawOnce {}
// SAFETY: see above.
unsafe impl Sync for CortexMRawOnce {}
// SAFETY: see above.
unsafe impl Send for CortexMSemaphore {}
// SAFETY: see above.
unsafe impl Sync for CortexMSemaphore {}
// SAFETY: see above.
unsafe impl Send for CortexMRawRwLock {}
// SAFETY: see above.
unsafe impl Sync for CortexMRawRwLock {}

/// Selected raw mutex type for Cortex-M builds.
pub type PlatformRawMutex = CortexMRawMutex;

/// Selected semaphore type for Cortex-M builds.
pub type PlatformSemaphore = CortexMSemaphore;

/// Selected raw once type for Cortex-M builds.
pub type PlatformRawOnce = CortexMRawOnce;

/// Selected raw rwlock type for Cortex-M builds.
pub type PlatformRawRwLock = CortexMRawRwLock;

/// Target-selected synchronization provider alias for Cortex-M builds.
pub type PlatformSync = CortexMSync;

/// Backend truth for the selected raw mutex implementation on Cortex-M.
pub const PLATFORM_RAW_MUTEX_IMPLEMENTATION: SyncImplementationKind =
    if cfg!(any(target_has_atomic = "8", target_has_atomic = "32"))
        || CORTEX_M_LOCAL_CRITICAL_SECTION_SYNC_SAFE
    {
        SyncImplementationKind::Emulated
    } else {
        SyncImplementationKind::Unsupported
    };

/// Backend truth for the selected raw once implementation on Cortex-M.
pub const PLATFORM_RAW_ONCE_IMPLEMENTATION: SyncImplementationKind =
    if cfg!(any(target_has_atomic = "8", target_has_atomic = "32"))
        || CORTEX_M_LOCAL_CRITICAL_SECTION_SYNC_SAFE
    {
        SyncImplementationKind::Emulated
    } else {
        SyncImplementationKind::Unsupported
    };

/// Backend truth for the selected raw rwlock implementation on Cortex-M.
pub const PLATFORM_RAW_RWLOCK_IMPLEMENTATION: SyncImplementationKind =
    if cfg!(any(target_has_atomic = "16", target_has_atomic = "32"))
        || CORTEX_M_LOCAL_CRITICAL_SECTION_SYNC_SAFE
    {
        SyncImplementationKind::Emulated
    } else {
        SyncImplementationKind::Unsupported
    };

/// Returns the process-wide Cortex-M synchronization provider handle.
#[must_use]
pub const fn system_sync() -> PlatformSync {
    PlatformSync::new()
}

#[cfg(any(
    not(any(target_has_atomic = "8", target_has_atomic = "32")),
    not(any(target_has_atomic = "16", target_has_atomic = "32"))
))]
#[derive(Debug, Clone)]
struct CortexMCriticalSection {
    primask: u32,
}

#[cfg(any(
    not(any(target_has_atomic = "8", target_has_atomic = "32")),
    not(any(target_has_atomic = "16", target_has_atomic = "32"))
))]
impl CortexMCriticalSection {
    #[inline]
    fn enter() -> Self {
        let primask: u32;
        // SAFETY: reading and updating PRIMASK is the architected way to enter a local
        // interrupt-masked critical section on Cortex-M. The saved value is restored on drop.
        unsafe {
            asm!("mrs {0}, PRIMASK", out(reg) primask, options(nomem, nostack, preserves_flags));
            asm!("cpsid i", options(nomem, nostack, preserves_flags));
        }
        compiler_fence(Ordering::SeqCst);
        Self { primask }
    }
}

#[cfg(any(
    not(any(target_has_atomic = "8", target_has_atomic = "32")),
    not(any(target_has_atomic = "16", target_has_atomic = "32"))
))]
impl Drop for CortexMCriticalSection {
    fn drop(&mut self) {
        compiler_fence(Ordering::SeqCst);
        // SAFETY: this restores the saved PRIMASK value captured when the critical section
        // began, preserving nesting and pre-existing interrupt masking state.
        unsafe {
            asm!(
                "msr PRIMASK, {0}",
                in(reg) self.primask,
                options(nomem, nostack, preserves_flags)
            );
        }
    }
}

#[cfg(any(
    not(any(target_has_atomic = "8", target_has_atomic = "32")),
    not(any(target_has_atomic = "16", target_has_atomic = "32"))
))]
#[inline]
fn with_local_critical_section<T>(f: impl FnOnce() -> T) -> T {
    let _guard = CortexMCriticalSection::enter();
    f()
}

#[inline]
const fn mutex_support_surface() -> MutexSupport {
    if cfg!(any(target_has_atomic = "8", target_has_atomic = "32")) {
        CORTEX_M_MUTEX_SUPPORT_ATOMIC
    } else if CORTEX_M_LOCAL_CRITICAL_SECTION_SYNC_SAFE {
        CORTEX_M_MUTEX_SUPPORT_CRITICAL
    } else {
        MutexSupport::unsupported()
    }
}

#[inline]
const fn once_support_surface() -> OnceSupport {
    if cfg!(any(target_has_atomic = "8", target_has_atomic = "32")) {
        CORTEX_M_ONCE_SUPPORT_ATOMIC
    } else if CORTEX_M_LOCAL_CRITICAL_SECTION_SYNC_SAFE {
        CORTEX_M_ONCE_SUPPORT_CRITICAL
    } else {
        OnceSupport::unsupported()
    }
}

#[inline]
const fn semaphore_support_surface() -> SemaphoreSupport {
    if cfg!(any(target_has_atomic = "16", target_has_atomic = "32")) {
        CORTEX_M_SEMAPHORE_SUPPORT_ATOMIC
    } else if CORTEX_M_LOCAL_CRITICAL_SECTION_SYNC_SAFE {
        CORTEX_M_SEMAPHORE_SUPPORT_CRITICAL
    } else {
        SemaphoreSupport::unsupported()
    }
}

#[inline]
const fn rwlock_support_surface() -> RwLockSupport {
    if cfg!(any(target_has_atomic = "16", target_has_atomic = "32")) {
        CORTEX_M_RWLOCK_SUPPORT_ATOMIC
    } else if CORTEX_M_LOCAL_CRITICAL_SECTION_SYNC_SAFE {
        CORTEX_M_RWLOCK_SUPPORT_CRITICAL
    } else {
        RwLockSupport::unsupported()
    }
}

impl CortexMSync {
    /// Creates a new Cortex-M synchronization provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl SyncBase for CortexMSync {
    fn support(&self) -> SyncSupport {
        SyncSupport {
            wait: WaitSupport::unsupported(),
            mutex: mutex_support_surface(),
            once: once_support_surface(),
            semaphore: semaphore_support_surface(),
            rwlock: rwlock_support_surface(),
        }
    }
}

impl WaitPrimitive for CortexMSync {
    fn support(&self) -> WaitSupport {
        WaitSupport::unsupported()
    }

    fn wait_while_equal(
        &self,
        _word: &AtomicU32,
        _expected: u32,
        _timeout: Option<Duration>,
    ) -> Result<WaitOutcome, SyncError> {
        Err(SyncError::unsupported())
    }

    fn wake_one(&self, _word: &AtomicU32) -> Result<usize, SyncError> {
        Err(SyncError::unsupported())
    }

    fn wake_all(&self, _word: &AtomicU32) -> Result<usize, SyncError> {
        Err(SyncError::unsupported())
    }
}

impl CortexMRawMutex {
    /// Creates a new unlocked Cortex-M raw mutex.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            #[cfg(target_has_atomic = "8")]
            state: AtomicU8::new(MUTEX_UNLOCKED),
            #[cfg(all(not(target_has_atomic = "8"), target_has_atomic = "32"))]
            state: AtomicU32::new(MUTEX_UNLOCKED_WORD),
            #[cfg(not(any(target_has_atomic = "8", target_has_atomic = "32")))]
            state: UnsafeCell::new(MUTEX_UNLOCKED),
        }
    }
}

// SAFETY: this mutex uses an atomic ownership word with acquire/release semantics and requires
// callers to uphold balanced unlock discipline through the raw contract.
unsafe impl RawMutex for CortexMRawMutex {
    fn support(&self) -> MutexSupport {
        mutex_support_surface()
    }

    fn lock(&self) -> Result<(), SyncError> {
        loop {
            if self.try_lock()? {
                return Ok(());
            }
            spin_loop();
        }
    }

    fn try_lock(&self) -> Result<bool, SyncError> {
        #[cfg(target_has_atomic = "8")]
        {
            Ok(self
                .state
                .compare_exchange(
                    MUTEX_UNLOCKED,
                    MUTEX_LOCKED,
                    Ordering::Acquire,
                    Ordering::Relaxed,
                )
                .is_ok())
        }

        #[cfg(all(not(target_has_atomic = "8"), target_has_atomic = "32"))]
        {
            Ok(self
                .state
                .compare_exchange(
                    MUTEX_UNLOCKED_WORD,
                    MUTEX_LOCKED_WORD,
                    Ordering::Acquire,
                    Ordering::Relaxed,
                )
                .is_ok())
        }

        #[cfg(not(any(target_has_atomic = "8", target_has_atomic = "32")))]
        {
            if !CORTEX_M_LOCAL_CRITICAL_SECTION_SYNC_SAFE {
                return Err(SyncError::unsupported());
            }

            Ok(with_local_critical_section(|| {
                // SAFETY: this state cell is only accessed while local interrupts are masked on
                // boards that explicitly promise that such a critical section serializes all
                // competing execution touching this primitive.
                let state = unsafe { &mut *self.state.get() };
                if *state == MUTEX_UNLOCKED {
                    *state = MUTEX_LOCKED;
                    true
                } else {
                    false
                }
            }))
        }
    }

    unsafe fn unlock_unchecked(&self) {
        #[cfg(target_has_atomic = "8")]
        {
            self.state.store(MUTEX_UNLOCKED, Ordering::Release);
        }

        #[cfg(all(not(target_has_atomic = "8"), target_has_atomic = "32"))]
        {
            self.state.store(MUTEX_UNLOCKED_WORD, Ordering::Release);
        }

        #[cfg(not(any(target_has_atomic = "8", target_has_atomic = "32")))]
        {
            if CORTEX_M_LOCAL_CRITICAL_SECTION_SYNC_SAFE {
                with_local_critical_section(|| {
                    // SAFETY: see `try_lock`; unlock participates in the same serialized access.
                    unsafe { *self.state.get() = MUTEX_UNLOCKED };
                });
            }
        }
    }
}

impl CortexMRawOnce {
    /// Creates a new uninitialized Cortex-M raw once primitive.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            #[cfg(target_has_atomic = "8")]
            state: AtomicU8::new(ONCE_UNINITIALIZED),
            #[cfg(all(not(target_has_atomic = "8"), target_has_atomic = "32"))]
            state: AtomicU32::new(ONCE_UNINITIALIZED_WORD),
            #[cfg(not(any(target_has_atomic = "8", target_has_atomic = "32")))]
            state: UnsafeCell::new(ONCE_UNINITIALIZED),
        }
    }
}

impl RawOnce for CortexMRawOnce {
    fn support(&self) -> OnceSupport {
        once_support_surface()
    }

    fn state(&self) -> OnceState {
        #[cfg(target_has_atomic = "8")]
        {
            match self.state.load(Ordering::Acquire) {
                ONCE_RUNNING => OnceState::Running,
                ONCE_COMPLETE => OnceState::Complete,
                _ => OnceState::Uninitialized,
            }
        }

        #[cfg(all(not(target_has_atomic = "8"), target_has_atomic = "32"))]
        {
            match self.state.load(Ordering::Acquire) {
                ONCE_RUNNING_WORD => OnceState::Running,
                ONCE_COMPLETE_WORD => OnceState::Complete,
                _ => OnceState::Uninitialized,
            }
        }

        #[cfg(not(any(target_has_atomic = "8", target_has_atomic = "32")))]
        {
            if !CORTEX_M_LOCAL_CRITICAL_SECTION_SYNC_SAFE {
                return OnceState::Uninitialized;
            }

            with_local_critical_section(|| {
                // SAFETY: state inspection is serialized by the local critical section.
                match unsafe { *self.state.get() } {
                    ONCE_RUNNING => OnceState::Running,
                    ONCE_COMPLETE => OnceState::Complete,
                    _ => OnceState::Uninitialized,
                }
            })
        }
    }

    fn begin(&self) -> Result<OnceBeginResult, SyncError> {
        #[cfg(target_has_atomic = "8")]
        {
            loop {
                match self.state.load(Ordering::Acquire) {
                    ONCE_COMPLETE => return Ok(OnceBeginResult::Complete),
                    ONCE_RUNNING => return Ok(OnceBeginResult::InProgress),
                    ONCE_UNINITIALIZED => {
                        if self
                            .state
                            .compare_exchange(
                                ONCE_UNINITIALIZED,
                                ONCE_RUNNING,
                                Ordering::Acquire,
                                Ordering::Relaxed,
                            )
                            .is_ok()
                        {
                            return Ok(OnceBeginResult::Initialize);
                        }
                    }
                    _ => return Err(SyncError::invalid()),
                }
            }
        }

        #[cfg(all(not(target_has_atomic = "8"), target_has_atomic = "32"))]
        {
            loop {
                match self.state.load(Ordering::Acquire) {
                    ONCE_COMPLETE_WORD => return Ok(OnceBeginResult::Complete),
                    ONCE_RUNNING_WORD => return Ok(OnceBeginResult::InProgress),
                    ONCE_UNINITIALIZED_WORD => {
                        if self
                            .state
                            .compare_exchange(
                                ONCE_UNINITIALIZED_WORD,
                                ONCE_RUNNING_WORD,
                                Ordering::Acquire,
                                Ordering::Relaxed,
                            )
                            .is_ok()
                        {
                            return Ok(OnceBeginResult::Initialize);
                        }
                    }
                    _ => return Err(SyncError::invalid()),
                }
            }
        }

        #[cfg(not(any(target_has_atomic = "8", target_has_atomic = "32")))]
        {
            if !CORTEX_M_LOCAL_CRITICAL_SECTION_SYNC_SAFE {
                return Err(SyncError::unsupported());
            }

            with_local_critical_section(|| {
                // SAFETY: state transition is serialized by the local critical section.
                let state = unsafe { &mut *self.state.get() };
                match *state {
                    ONCE_COMPLETE => Ok(OnceBeginResult::Complete),
                    ONCE_RUNNING => Ok(OnceBeginResult::InProgress),
                    ONCE_UNINITIALIZED => {
                        *state = ONCE_RUNNING;
                        Ok(OnceBeginResult::Initialize)
                    }
                    _ => Err(SyncError::invalid()),
                }
            })
        }
    }

    fn wait(&self) -> Result<(), SyncError> {
        #[cfg(target_has_atomic = "8")]
        {
            while self.state.load(Ordering::Acquire) == ONCE_RUNNING {
                spin_loop();
            }
            Ok(())
        }

        #[cfg(all(not(target_has_atomic = "8"), target_has_atomic = "32"))]
        {
            while self.state.load(Ordering::Acquire) == ONCE_RUNNING_WORD {
                spin_loop();
            }
            Ok(())
        }

        #[cfg(not(any(target_has_atomic = "8", target_has_atomic = "32")))]
        {
            if !CORTEX_M_LOCAL_CRITICAL_SECTION_SYNC_SAFE {
                return Err(SyncError::unsupported());
            }

            while with_local_critical_section(|| {
                // SAFETY: state inspection is serialized by the local critical section.
                unsafe { *self.state.get() == ONCE_RUNNING }
            }) {
                spin_loop();
            }
            Ok(())
        }
    }

    unsafe fn complete_unchecked(&self) {
        #[cfg(target_has_atomic = "8")]
        {
            self.state.store(ONCE_COMPLETE, Ordering::Release);
        }

        #[cfg(all(not(target_has_atomic = "8"), target_has_atomic = "32"))]
        {
            self.state.store(ONCE_COMPLETE_WORD, Ordering::Release);
        }

        #[cfg(not(any(target_has_atomic = "8", target_has_atomic = "32")))]
        {
            if CORTEX_M_LOCAL_CRITICAL_SECTION_SYNC_SAFE {
                with_local_critical_section(|| {
                    // SAFETY: completion is serialized by the local critical section.
                    unsafe { *self.state.get() = ONCE_COMPLETE };
                });
            }
        }
    }

    unsafe fn reset_unchecked(&self) {
        #[cfg(target_has_atomic = "8")]
        {
            self.state.store(ONCE_UNINITIALIZED, Ordering::Release);
        }

        #[cfg(all(not(target_has_atomic = "8"), target_has_atomic = "32"))]
        {
            self.state.store(ONCE_UNINITIALIZED_WORD, Ordering::Release);
        }

        #[cfg(not(any(target_has_atomic = "8", target_has_atomic = "32")))]
        {
            if CORTEX_M_LOCAL_CRITICAL_SECTION_SYNC_SAFE {
                with_local_critical_section(|| {
                    // SAFETY: reset is serialized by the local critical section.
                    unsafe { *self.state.get() = ONCE_UNINITIALIZED };
                });
            }
        }
    }
}

impl CortexMSemaphore {
    /// Creates a new Cortex-M semaphore with a bounded permit range.
    ///
    /// # Errors
    ///
    /// Returns an error when the requested bounds cannot be represented honestly on the
    /// selected target.
    pub const fn new(initial: u32, max: u32) -> Result<Self, SyncError> {
        #[cfg(target_has_atomic = "16")]
        {
            if initial > max || max > u16::MAX as u32 {
                return Err(SyncError::invalid());
            }

            Ok(Self {
                permits: AtomicU16::new(initial as u16),
                max: max as u16,
            })
        }

        #[cfg(all(not(target_has_atomic = "16"), target_has_atomic = "32"))]
        {
            if initial > max {
                return Err(SyncError::invalid());
            }

            Ok(Self {
                permits: AtomicU32::new(initial),
                max,
            })
        }

        #[cfg(not(any(target_has_atomic = "16", target_has_atomic = "32")))]
        {
            if initial > max {
                return Err(SyncError::invalid());
            }

            if !CORTEX_M_LOCAL_CRITICAL_SECTION_SYNC_SAFE {
                return Err(SyncError::unsupported());
            }

            Ok(Self {
                permits: UnsafeCell::new(initial),
                max,
            })
        }
    }
}

impl RawSemaphore for CortexMSemaphore {
    fn support(&self) -> SemaphoreSupport {
        semaphore_support_surface()
    }

    fn acquire(&self) -> Result<(), SyncError> {
        loop {
            if self.try_acquire()? {
                return Ok(());
            }
            spin_loop();
        }
    }

    fn try_acquire(&self) -> Result<bool, SyncError> {
        #[cfg(target_has_atomic = "16")]
        {
            loop {
                let permits = self.permits.load(Ordering::Acquire);
                if permits == 0 {
                    return Ok(false);
                }

                if self
                    .permits
                    .compare_exchange(permits, permits - 1, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    return Ok(true);
                }
            }
        }

        #[cfg(all(not(target_has_atomic = "16"), target_has_atomic = "32"))]
        {
            loop {
                let permits = self.permits.load(Ordering::Acquire);
                if permits == 0 {
                    return Ok(false);
                }

                if self
                    .permits
                    .compare_exchange(permits, permits - 1, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    return Ok(true);
                }
            }
        }

        #[cfg(not(any(target_has_atomic = "16", target_has_atomic = "32")))]
        {
            if !CORTEX_M_LOCAL_CRITICAL_SECTION_SYNC_SAFE {
                return Err(SyncError::unsupported());
            }

            Ok(with_local_critical_section(|| {
                // SAFETY: permit accounting is serialized by the local critical section.
                let permits = unsafe { &mut *self.permits.get() };
                if *permits == 0 {
                    false
                } else {
                    *permits -= 1;
                    true
                }
            }))
        }
    }

    fn release(&self, permits: u32) -> Result<(), SyncError> {
        #[cfg(target_has_atomic = "16")]
        {
            if permits > u32::from(u16::MAX) {
                return Err(SyncError::overflow());
            }

            let permits = permits as u16;
            loop {
                let current = self.permits.load(Ordering::Acquire);
                let next = current
                    .checked_add(permits)
                    .filter(|next| *next <= self.max)
                    .ok_or_else(SyncError::overflow)?;

                if self
                    .permits
                    .compare_exchange(current, next, Ordering::Release, Ordering::Relaxed)
                    .is_ok()
                {
                    return Ok(());
                }
            }
        }

        #[cfg(all(not(target_has_atomic = "16"), target_has_atomic = "32"))]
        {
            loop {
                let current = self.permits.load(Ordering::Acquire);
                let next = current
                    .checked_add(permits)
                    .filter(|next| *next <= self.max)
                    .ok_or_else(SyncError::overflow)?;

                if self
                    .permits
                    .compare_exchange(current, next, Ordering::Release, Ordering::Relaxed)
                    .is_ok()
                {
                    return Ok(());
                }
            }
        }

        #[cfg(not(any(target_has_atomic = "16", target_has_atomic = "32")))]
        {
            if !CORTEX_M_LOCAL_CRITICAL_SECTION_SYNC_SAFE {
                return Err(SyncError::unsupported());
            }

            with_local_critical_section(|| {
                // SAFETY: permit accounting is serialized by the local critical section.
                let current = unsafe { &mut *self.permits.get() };
                let next = (*current)
                    .checked_add(permits)
                    .filter(|next| *next <= self.max)
                    .ok_or_else(SyncError::overflow)?;
                *current = next;
                Ok(())
            })
        }
    }

    fn max_permits(&self) -> u32 {
        #[cfg(target_has_atomic = "16")]
        {
            u32::from(self.max)
        }

        #[cfg(all(not(target_has_atomic = "16"), target_has_atomic = "32"))]
        {
            self.max
        }

        #[cfg(not(any(target_has_atomic = "16", target_has_atomic = "32")))]
        {
            self.max
        }
    }
}

impl CortexMRawRwLock {
    /// Creates a new unlocked Cortex-M raw rwlock.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            #[cfg(target_has_atomic = "16")]
            state: AtomicU16::new(0),
            #[cfg(all(not(target_has_atomic = "16"), target_has_atomic = "32"))]
            state: AtomicU32::new(0),
            #[cfg(not(any(target_has_atomic = "16", target_has_atomic = "32")))]
            state: UnsafeCell::new(0),
        }
    }
}

// SAFETY: this rwlock uses an atomic state word to serialize readers and writers with the raw
// acquire/release semantics required by the contract.
unsafe impl RawRwLock for CortexMRawRwLock {
    fn support(&self) -> RwLockSupport {
        rwlock_support_surface()
    }

    fn read_lock(&self) -> Result<(), SyncError> {
        loop {
            if self.try_read_lock()? {
                return Ok(());
            }
            spin_loop();
        }
    }

    fn try_read_lock(&self) -> Result<bool, SyncError> {
        #[cfg(target_has_atomic = "16")]
        {
            loop {
                let state = self.state.load(Ordering::Acquire);
                if state & RWLOCK_WRITER != 0 {
                    return Ok(false);
                }

                let readers = state & RWLOCK_READERS_MASK;
                if readers == RWLOCK_READERS_MASK {
                    return Err(SyncError::overflow());
                }

                if self
                    .state
                    .compare_exchange(state, state + 1, Ordering::Acquire, Ordering::Relaxed)
                    .is_ok()
                {
                    return Ok(true);
                }
            }
        }

        #[cfg(all(not(target_has_atomic = "16"), target_has_atomic = "32"))]
        {
            loop {
                let state = self.state.load(Ordering::Acquire);
                if state & RWLOCK_WRITER_WORD != 0 {
                    return Ok(false);
                }

                let readers = state & RWLOCK_READERS_MASK_WORD;
                if readers == RWLOCK_READERS_MASK_WORD {
                    return Err(SyncError::overflow());
                }

                if self
                    .state
                    .compare_exchange(state, state + 1, Ordering::Acquire, Ordering::Relaxed)
                    .is_ok()
                {
                    return Ok(true);
                }
            }
        }

        #[cfg(not(any(target_has_atomic = "16", target_has_atomic = "32")))]
        {
            if !CORTEX_M_LOCAL_CRITICAL_SECTION_SYNC_SAFE {
                return Err(SyncError::unsupported());
            }

            with_local_critical_section(|| {
                // SAFETY: rwlock state is serialized by the local critical section.
                let state = unsafe { &mut *self.state.get() };
                if *state & RWLOCK_WRITER_WORD != 0 {
                    return Ok(false);
                }

                let readers = *state & RWLOCK_READERS_MASK_WORD;
                if readers == RWLOCK_READERS_MASK_WORD {
                    return Err(SyncError::overflow());
                }

                *state += 1;
                Ok(true)
            })
        }
    }

    fn write_lock(&self) -> Result<(), SyncError> {
        loop {
            if self.try_write_lock()? {
                return Ok(());
            }
            spin_loop();
        }
    }

    fn try_write_lock(&self) -> Result<bool, SyncError> {
        #[cfg(target_has_atomic = "16")]
        {
            Ok(self
                .state
                .compare_exchange(0, RWLOCK_WRITER, Ordering::Acquire, Ordering::Relaxed)
                .is_ok())
        }

        #[cfg(all(not(target_has_atomic = "16"), target_has_atomic = "32"))]
        {
            Ok(self
                .state
                .compare_exchange(0, RWLOCK_WRITER_WORD, Ordering::Acquire, Ordering::Relaxed)
                .is_ok())
        }

        #[cfg(not(any(target_has_atomic = "16", target_has_atomic = "32")))]
        {
            if !CORTEX_M_LOCAL_CRITICAL_SECTION_SYNC_SAFE {
                return Err(SyncError::unsupported());
            }

            Ok(with_local_critical_section(|| {
                // SAFETY: rwlock state is serialized by the local critical section.
                let state = unsafe { &mut *self.state.get() };
                if *state == 0 {
                    *state = RWLOCK_WRITER_WORD;
                    true
                } else {
                    false
                }
            }))
        }
    }

    unsafe fn read_unlock_unchecked(&self) {
        #[cfg(target_has_atomic = "16")]
        {
            self.state.fetch_sub(1, Ordering::Release);
        }

        #[cfg(all(not(target_has_atomic = "16"), target_has_atomic = "32"))]
        {
            self.state.fetch_sub(1, Ordering::Release);
        }

        #[cfg(not(any(target_has_atomic = "16", target_has_atomic = "32")))]
        {
            if CORTEX_M_LOCAL_CRITICAL_SECTION_SYNC_SAFE {
                with_local_critical_section(|| {
                    // SAFETY: unlock participates in the same serialized state mutation.
                    unsafe { *self.state.get() -= 1 };
                });
            }
        }
    }

    unsafe fn write_unlock_unchecked(&self) {
        #[cfg(target_has_atomic = "16")]
        {
            self.state.store(0, Ordering::Release);
        }

        #[cfg(all(not(target_has_atomic = "16"), target_has_atomic = "32"))]
        {
            self.state.store(0, Ordering::Release);
        }

        #[cfg(not(any(target_has_atomic = "16", target_has_atomic = "32")))]
        {
            if CORTEX_M_LOCAL_CRITICAL_SECTION_SYNC_SAFE {
                with_local_critical_section(|| {
                    // SAFETY: unlock participates in the same serialized state mutation.
                    unsafe { *self.state.get() = 0 };
                });
            }
        }
    }
}
