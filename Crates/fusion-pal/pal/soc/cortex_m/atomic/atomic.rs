//! Cortex-M bare-metal atomic backend.
//!
//! This backend exposes one honest 32-bit atomic-word contract when the selected target has
//! native 32-bit atomics. A local critical-section fallback exists only for boards that
//! explicitly declare that path truthful; current supported boards do not.

#[cfg(not(target_has_atomic = "32"))]
use core::arch::asm;
#[cfg(not(target_has_atomic = "32"))]
use core::cell::UnsafeCell;
#[cfg(not(target_has_atomic = "32"))]
use core::sync::atomic::compiler_fence;
use core::sync::atomic::{
    AtomicU32,
    Ordering,
};
use core::time::Duration;

use crate::contract::pal::runtime::atomic::{
    AtomicBase,
    AtomicCompareExchangeOutcome32,
    AtomicError,
    AtomicFallbackKind,
    AtomicImplementationKind,
    AtomicSupport,
    AtomicWaitOutcome,
    AtomicWaitWord32Support,
    AtomicWord32,
    AtomicWord32Caps,
    AtomicWord32Support,
};

const CORTEX_M_LOCAL_CRITICAL_SECTION_ATOMIC_SAFE: bool =
    crate::pal::soc::cortex_m::hal::soc::board::LOCAL_CRITICAL_SECTION_SYNC_SAFE;

const CORTEX_M_WORD32_SUPPORT_ATOMIC: AtomicWord32Support = AtomicWord32Support {
    caps: AtomicWord32Caps::LOAD
        .union(AtomicWord32Caps::STORE)
        .union(AtomicWord32Caps::SWAP)
        .union(AtomicWord32Caps::COMPARE_EXCHANGE)
        .union(AtomicWord32Caps::FETCH_ADD)
        .union(AtomicWord32Caps::FETCH_SUB)
        .union(AtomicWord32Caps::FETCH_AND)
        .union(AtomicWord32Caps::FETCH_OR)
        .union(AtomicWord32Caps::FETCH_XOR)
        .union(AtomicWord32Caps::STATIC_INIT),
    implementation: AtomicImplementationKind::Native,
    fallback: AtomicFallbackKind::None,
};

const CORTEX_M_WORD32_SUPPORT_CRITICAL: AtomicWord32Support = AtomicWord32Support {
    caps: AtomicWord32Caps::LOAD
        .union(AtomicWord32Caps::STORE)
        .union(AtomicWord32Caps::SWAP)
        .union(AtomicWord32Caps::COMPARE_EXCHANGE)
        .union(AtomicWord32Caps::FETCH_ADD)
        .union(AtomicWord32Caps::FETCH_SUB)
        .union(AtomicWord32Caps::FETCH_AND)
        .union(AtomicWord32Caps::FETCH_OR)
        .union(AtomicWord32Caps::FETCH_XOR)
        .union(AtomicWord32Caps::STATIC_INIT),
    implementation: AtomicImplementationKind::Emulated,
    fallback: AtomicFallbackKind::CriticalSection,
};

/// Cortex-M atomic provider handle.
#[derive(Debug, Clone, Copy, Default)]
pub struct CortexMAtomic;

/// Cortex-M 32-bit atomic word backed by the selected local primitive.
#[derive(Debug)]
pub struct CortexMAtomicWord32 {
    #[cfg(target_has_atomic = "32")]
    inner: AtomicU32,
    #[cfg(not(target_has_atomic = "32"))]
    inner: UnsafeCell<u32>,
}

// SAFETY: this type coordinates all interior mutation through either native atomics or one local
// interrupt-masked critical section on platforms that explicitly allow it.
unsafe impl Send for CortexMAtomicWord32 {}
// SAFETY: see above.
unsafe impl Sync for CortexMAtomicWord32 {}

/// Target-selected atomic provider alias for Cortex-M builds.
pub type PlatformAtomic = CortexMAtomic;

/// Selected 32-bit atomic word type for Cortex-M builds.
pub type PlatformAtomicWord32 = CortexMAtomicWord32;

/// Backend truth for the selected 32-bit atomic-word implementation on Cortex-M.
pub const PLATFORM_ATOMIC_WORD32_IMPLEMENTATION: AtomicImplementationKind =
    if cfg!(target_has_atomic = "32") {
        AtomicImplementationKind::Native
    } else if CORTEX_M_LOCAL_CRITICAL_SECTION_ATOMIC_SAFE {
        AtomicImplementationKind::Emulated
    } else {
        AtomicImplementationKind::Unsupported
    };

/// Backend truth for the selected 32-bit atomic wait/wake implementation on Cortex-M.
pub const PLATFORM_ATOMIC_WAIT_WORD32_IMPLEMENTATION: AtomicImplementationKind =
    AtomicImplementationKind::Unsupported;

/// Returns the process-wide Cortex-M atomic provider handle.
#[must_use]
pub const fn system_atomic() -> PlatformAtomic {
    PlatformAtomic::new()
}

#[cfg(not(target_has_atomic = "32"))]
#[derive(Debug, Clone)]
struct CortexMCriticalSection {
    primask: u32,
}

#[cfg(not(target_has_atomic = "32"))]
impl CortexMCriticalSection {
    #[inline]
    fn enter() -> Self {
        let primask: u32;
        unsafe {
            asm!("mrs {0}, PRIMASK", out(reg) primask, options(nomem, nostack, preserves_flags));
            asm!("cpsid i", options(nomem, nostack, preserves_flags));
        }
        compiler_fence(Ordering::SeqCst);
        Self { primask }
    }
}

#[cfg(not(target_has_atomic = "32"))]
impl Drop for CortexMCriticalSection {
    fn drop(&mut self) {
        compiler_fence(Ordering::SeqCst);
        unsafe {
            asm!(
                "msr PRIMASK, {0}",
                in(reg) self.primask,
                options(nomem, nostack, preserves_flags)
            );
        }
    }
}

#[cfg(not(target_has_atomic = "32"))]
#[inline]
fn with_local_critical_section<T>(f: impl FnOnce() -> T) -> T {
    let _guard = CortexMCriticalSection::enter();
    f()
}

#[inline]
const fn word32_support_surface() -> AtomicWord32Support {
    if cfg!(target_has_atomic = "32") {
        CORTEX_M_WORD32_SUPPORT_ATOMIC
    } else if CORTEX_M_LOCAL_CRITICAL_SECTION_ATOMIC_SAFE {
        CORTEX_M_WORD32_SUPPORT_CRITICAL
    } else {
        AtomicWord32Support::unsupported()
    }
}

impl CortexMAtomic {
    /// Creates a new Cortex-M atomic provider handle.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl AtomicBase for CortexMAtomic {
    type Word32 = CortexMAtomicWord32;

    fn support(&self) -> AtomicSupport {
        AtomicSupport {
            word32: word32_support_surface(),
            wait_word32: AtomicWaitWord32Support::unsupported(),
        }
    }

    fn new_word32(&self, initial: u32) -> Result<Self::Word32, AtomicError> {
        if matches!(
            word32_support_surface().implementation,
            AtomicImplementationKind::Unsupported
        ) {
            return Err(AtomicError::unsupported());
        }

        Ok(CortexMAtomicWord32::new(initial))
    }
}

impl CortexMAtomicWord32 {
    /// Creates a new Cortex-M 32-bit atomic word.
    #[must_use]
    pub const fn new(initial: u32) -> Self {
        Self {
            #[cfg(target_has_atomic = "32")]
            inner: AtomicU32::new(initial),
            #[cfg(not(target_has_atomic = "32"))]
            inner: UnsafeCell::new(initial),
        }
    }
}

impl Default for CortexMAtomicWord32 {
    fn default() -> Self {
        Self::new(0)
    }
}

impl AtomicWord32 for CortexMAtomicWord32 {
    fn support(&self) -> AtomicWord32Support {
        word32_support_surface()
    }

    fn wait_support(&self) -> AtomicWaitWord32Support {
        AtomicWaitWord32Support::unsupported()
    }

    fn load(&self, ordering: Ordering) -> Result<u32, AtomicError> {
        validate_load_ordering(ordering)?;
        #[cfg(target_has_atomic = "32")]
        {
            return Ok(self.inner.load(ordering));
        }

        #[cfg(not(target_has_atomic = "32"))]
        {
            let _ = ordering;
            if !CORTEX_M_LOCAL_CRITICAL_SECTION_ATOMIC_SAFE {
                return Err(AtomicError::unsupported());
            }
            return Ok(with_local_critical_section(|| unsafe { *self.inner.get() }));
        }
    }

    fn store(&self, value: u32, ordering: Ordering) -> Result<(), AtomicError> {
        validate_store_ordering(ordering)?;
        #[cfg(target_has_atomic = "32")]
        {
            self.inner.store(value, ordering);
            return Ok(());
        }

        #[cfg(not(target_has_atomic = "32"))]
        {
            let _ = ordering;
            if !CORTEX_M_LOCAL_CRITICAL_SECTION_ATOMIC_SAFE {
                return Err(AtomicError::unsupported());
            }
            with_local_critical_section(|| unsafe {
                *self.inner.get() = value;
            });
            Ok(())
        }
    }

    fn swap(&self, value: u32, ordering: Ordering) -> Result<u32, AtomicError> {
        validate_rmw_ordering(ordering)?;
        #[cfg(target_has_atomic = "32")]
        {
            return Ok(self.inner.swap(value, ordering));
        }

        #[cfg(not(target_has_atomic = "32"))]
        {
            let _ = ordering;
            if !CORTEX_M_LOCAL_CRITICAL_SECTION_ATOMIC_SAFE {
                return Err(AtomicError::unsupported());
            }
            return Ok(with_local_critical_section(|| unsafe {
                let previous = *self.inner.get();
                *self.inner.get() = value;
                previous
            }));
        }
    }

    fn compare_exchange(
        &self,
        current: u32,
        new: u32,
        success: Ordering,
        failure: Ordering,
    ) -> Result<AtomicCompareExchangeOutcome32, AtomicError> {
        validate_compare_exchange_orderings(success, failure)?;
        #[cfg(target_has_atomic = "32")]
        {
            return Ok(
                match self.inner.compare_exchange(current, new, success, failure) {
                    Ok(_) => AtomicCompareExchangeOutcome32::Exchanged,
                    Err(observed) => AtomicCompareExchangeOutcome32::Mismatch(observed),
                },
            );
        }

        #[cfg(not(target_has_atomic = "32"))]
        {
            let _ = success;
            let _ = failure;
            if !CORTEX_M_LOCAL_CRITICAL_SECTION_ATOMIC_SAFE {
                return Err(AtomicError::unsupported());
            }
            return Ok(with_local_critical_section(|| unsafe {
                let observed = *self.inner.get();
                if observed == current {
                    *self.inner.get() = new;
                    AtomicCompareExchangeOutcome32::Exchanged
                } else {
                    AtomicCompareExchangeOutcome32::Mismatch(observed)
                }
            }));
        }
    }

    fn fetch_add(&self, value: u32, ordering: Ordering) -> Result<u32, AtomicError> {
        validate_rmw_ordering(ordering)?;
        #[cfg(target_has_atomic = "32")]
        {
            return Ok(self.inner.fetch_add(value, ordering));
        }

        #[cfg(not(target_has_atomic = "32"))]
        {
            let _ = ordering;
            if !CORTEX_M_LOCAL_CRITICAL_SECTION_ATOMIC_SAFE {
                return Err(AtomicError::unsupported());
            }
            return Ok(with_local_critical_section(|| unsafe {
                let previous = *self.inner.get();
                *self.inner.get() = previous.wrapping_add(value);
                previous
            }));
        }
    }

    fn fetch_sub(&self, value: u32, ordering: Ordering) -> Result<u32, AtomicError> {
        validate_rmw_ordering(ordering)?;
        #[cfg(target_has_atomic = "32")]
        {
            return Ok(self.inner.fetch_sub(value, ordering));
        }

        #[cfg(not(target_has_atomic = "32"))]
        {
            let _ = ordering;
            if !CORTEX_M_LOCAL_CRITICAL_SECTION_ATOMIC_SAFE {
                return Err(AtomicError::unsupported());
            }
            return Ok(with_local_critical_section(|| unsafe {
                let previous = *self.inner.get();
                *self.inner.get() = previous.wrapping_sub(value);
                previous
            }));
        }
    }

    fn fetch_and(&self, value: u32, ordering: Ordering) -> Result<u32, AtomicError> {
        validate_rmw_ordering(ordering)?;
        #[cfg(target_has_atomic = "32")]
        {
            return Ok(self.inner.fetch_and(value, ordering));
        }

        #[cfg(not(target_has_atomic = "32"))]
        {
            let _ = ordering;
            if !CORTEX_M_LOCAL_CRITICAL_SECTION_ATOMIC_SAFE {
                return Err(AtomicError::unsupported());
            }
            return Ok(with_local_critical_section(|| unsafe {
                let previous = *self.inner.get();
                *self.inner.get() = previous & value;
                previous
            }));
        }
    }

    fn fetch_or(&self, value: u32, ordering: Ordering) -> Result<u32, AtomicError> {
        validate_rmw_ordering(ordering)?;
        #[cfg(target_has_atomic = "32")]
        {
            return Ok(self.inner.fetch_or(value, ordering));
        }

        #[cfg(not(target_has_atomic = "32"))]
        {
            let _ = ordering;
            if !CORTEX_M_LOCAL_CRITICAL_SECTION_ATOMIC_SAFE {
                return Err(AtomicError::unsupported());
            }
            return Ok(with_local_critical_section(|| unsafe {
                let previous = *self.inner.get();
                *self.inner.get() = previous | value;
                previous
            }));
        }
    }

    fn fetch_xor(&self, value: u32, ordering: Ordering) -> Result<u32, AtomicError> {
        validate_rmw_ordering(ordering)?;
        #[cfg(target_has_atomic = "32")]
        {
            return Ok(self.inner.fetch_xor(value, ordering));
        }

        #[cfg(not(target_has_atomic = "32"))]
        {
            let _ = ordering;
            if !CORTEX_M_LOCAL_CRITICAL_SECTION_ATOMIC_SAFE {
                return Err(AtomicError::unsupported());
            }
            return Ok(with_local_critical_section(|| unsafe {
                let previous = *self.inner.get();
                *self.inner.get() = previous ^ value;
                previous
            }));
        }
    }

    fn wait_while_equal(
        &self,
        _expected: u32,
        _timeout: Option<Duration>,
    ) -> Result<AtomicWaitOutcome, AtomicError> {
        Err(AtomicError::unsupported())
    }

    fn wake_one(&self) -> Result<usize, AtomicError> {
        Err(AtomicError::unsupported())
    }

    fn wake_all(&self) -> Result<usize, AtomicError> {
        Err(AtomicError::unsupported())
    }
}

const fn validate_load_ordering(ordering: Ordering) -> Result<(), AtomicError> {
    match ordering {
        Ordering::Relaxed | Ordering::Acquire | Ordering::SeqCst => Ok(()),
        Ordering::Release | Ordering::AcqRel => Err(AtomicError::invalid()),
        _ => Err(AtomicError::invalid()),
    }
}

const fn validate_store_ordering(ordering: Ordering) -> Result<(), AtomicError> {
    match ordering {
        Ordering::Relaxed | Ordering::Release | Ordering::SeqCst => Ok(()),
        Ordering::Acquire | Ordering::AcqRel => Err(AtomicError::invalid()),
        _ => Err(AtomicError::invalid()),
    }
}

const fn validate_rmw_ordering(ordering: Ordering) -> Result<(), AtomicError> {
    match ordering {
        Ordering::Relaxed
        | Ordering::Acquire
        | Ordering::Release
        | Ordering::AcqRel
        | Ordering::SeqCst => Ok(()),
        _ => Err(AtomicError::invalid()),
    }
}

const fn validate_compare_exchange_orderings(
    success: Ordering,
    failure: Ordering,
) -> Result<(), AtomicError> {
    if validate_rmw_ordering(success).is_err() {
        return Err(AtomicError::invalid());
    }

    match (success, failure) {
        (Ordering::Relaxed, Ordering::Relaxed)
        | (Ordering::Acquire, Ordering::Relaxed | Ordering::Acquire)
        | (Ordering::Release, Ordering::Relaxed)
        | (Ordering::AcqRel, Ordering::Relaxed | Ordering::Acquire)
        | (Ordering::SeqCst, Ordering::Relaxed | Ordering::Acquire | Ordering::SeqCst) => Ok(()),
        _ => Err(AtomicError::invalid()),
    }
}
