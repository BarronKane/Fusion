//! Backend-neutral unsupported atomic implementations.
//!
//! These types intentionally realize the fusion-pal atomic contracts in an always-unsupported
//! form. They are useful for hosted stubs and any other backend path that needs to report truthful
//! absence of runtime atomic surfaces without fabricating behavior.

use core::sync::atomic::{
    AtomicU32,
    Ordering,
};
use core::time::Duration;

use super::{
    AtomicBase,
    AtomicCompareExchangeOutcome32,
    AtomicError,
    AtomicSupport,
    AtomicWaitOutcome,
    AtomicWaitWord32Support,
    AtomicWord32,
    AtomicWord32Support,
};

/// Unsupported atomic provider placeholder.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedAtomic;

/// Unsupported 32-bit atomic word placeholder.
#[derive(Debug)]
pub struct UnsupportedAtomicWord32 {
    _state: AtomicU32,
}

impl UnsupportedAtomic {
    /// Creates a new unsupported atomic provider placeholder.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl AtomicBase for UnsupportedAtomic {
    type Word32 = UnsupportedAtomicWord32;

    fn support(&self) -> AtomicSupport {
        AtomicSupport::unsupported()
    }

    fn new_word32(&self, _initial: u32) -> Result<Self::Word32, AtomicError> {
        Err(AtomicError::unsupported())
    }
}

impl UnsupportedAtomicWord32 {
    /// Creates a new unsupported 32-bit atomic word placeholder.
    #[must_use]
    pub const fn new(initial: u32) -> Self {
        Self {
            _state: AtomicU32::new(initial),
        }
    }
}

impl Default for UnsupportedAtomicWord32 {
    fn default() -> Self {
        Self::new(0)
    }
}

impl AtomicWord32 for UnsupportedAtomicWord32 {
    fn support(&self) -> AtomicWord32Support {
        AtomicWord32Support::unsupported()
    }

    fn wait_support(&self) -> AtomicWaitWord32Support {
        AtomicWaitWord32Support::unsupported()
    }

    fn load(&self, _ordering: Ordering) -> Result<u32, AtomicError> {
        Err(AtomicError::unsupported())
    }

    fn store(&self, _value: u32, _ordering: Ordering) -> Result<(), AtomicError> {
        Err(AtomicError::unsupported())
    }

    fn swap(&self, _value: u32, _ordering: Ordering) -> Result<u32, AtomicError> {
        Err(AtomicError::unsupported())
    }

    fn compare_exchange(
        &self,
        _current: u32,
        _new: u32,
        _success: Ordering,
        _failure: Ordering,
    ) -> Result<AtomicCompareExchangeOutcome32, AtomicError> {
        Err(AtomicError::unsupported())
    }

    fn fetch_add(&self, _value: u32, _ordering: Ordering) -> Result<u32, AtomicError> {
        Err(AtomicError::unsupported())
    }

    fn fetch_sub(&self, _value: u32, _ordering: Ordering) -> Result<u32, AtomicError> {
        Err(AtomicError::unsupported())
    }

    fn fetch_and(&self, _value: u32, _ordering: Ordering) -> Result<u32, AtomicError> {
        Err(AtomicError::unsupported())
    }

    fn fetch_or(&self, _value: u32, _ordering: Ordering) -> Result<u32, AtomicError> {
        Err(AtomicError::unsupported())
    }

    fn fetch_xor(&self, _value: u32, _ordering: Ordering) -> Result<u32, AtomicError> {
        Err(AtomicError::unsupported())
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
