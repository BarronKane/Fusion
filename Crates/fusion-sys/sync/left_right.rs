//! Left-right-inspired read-mostly table primitive.
//!
//! Note: While I was soft speccing this in a general sense, I came across a crate
//! by some very talented developers:
//!
//! https://github.com/jonhoo/left-right.git
//!
//! I did not read the source of this crate, but I came across by way of a tech talk that
//! broke down the concept of left-right synchronization as apposed to mutexes and rwlock.
//! This is my own implementation that, I believe, effectively implements the idea.
//!
//! This is a deliberately narrow synchronization surface for lookup tables and similar
//! read-mostly structures where readers should stay cheap and the writer can afford to pay
//! the quiescence bill. Each reader slot owns one cacheline-padded counter. A read begins by
//! flipping that slot's counter from even to odd, snapshots the currently active replica, and
//! flips the counter back to even on drop.
//!
//! Writers stage updates into the inactive replica, publish the alternate side atomically, and
//! then wait until every reader slot is quiescent before considering the old replica writable
//! again. That is intentionally conservative: readers that arrive immediately after publication
//! still extend the quiescence window. For hardware lookup tables and other read-mostly state,
//! that trade usually beats building a more theatrical epoch system too early.
//!
//! One detail is worth saying out loud because Rust will happily let people lie to themselves:
//! this primitive uses an atomic replica selector, not an atomic self-pointer. A self-referential
//! pointer inside a movable `no_std` object is a wonderfully compact way to ship UB with good
//! documentation.

use core::cell::UnsafeCell;
use core::hint::spin_loop;
use core::ops::Deref;
use core::sync::atomic::{AtomicUsize, Ordering};

use fusion_pal::sys::cpu::CachePadded;

use super::{SyncError, ThinMutex};

const LEFT_RIGHT_ACTIVE_LEFT: usize = 0;
const LEFT_RIGHT_ACTIVE_RIGHT: usize = 1;

#[derive(Debug)]
struct LeftRightReaderCounter {
    state: AtomicUsize,
}

impl LeftRightReaderCounter {
    const fn new() -> Self {
        Self {
            state: AtomicUsize::new(0),
        }
    }

    fn begin(&self) -> Result<(), SyncError> {
        let mut current = self.state.load(Ordering::Acquire);
        loop {
            if current & 1 != 0 {
                return Err(SyncError::busy());
            }

            match self.state.compare_exchange_weak(
                current,
                current.wrapping_add(1),
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Ok(()),
                Err(observed) => current = observed,
            }
        }
    }

    fn finish(&self) {
        self.state.fetch_add(1, Ordering::Release);
    }

    fn is_quiescent(&self) -> bool {
        self.state.load(Ordering::Acquire) & 1 == 0
    }
}

/// Read-mostly dual-replica table with per-reader cache-local counters.
#[derive(Debug)]
pub struct LeftRight<T, const READERS: usize> {
    writer: ThinMutex,
    active_side: AtomicUsize,
    left: UnsafeCell<T>,
    right: UnsafeCell<T>,
    readers: [CachePadded<LeftRightReaderCounter>; READERS],
}

unsafe impl<T: Send, const READERS: usize> Send for LeftRight<T, READERS> {}
unsafe impl<T: Send + Sync, const READERS: usize> Sync for LeftRight<T, READERS> {}

impl<T: Clone, const READERS: usize> LeftRight<T, READERS> {
    /// Creates a new left-right table with identical initial replicas.
    #[must_use]
    pub fn new(value: T) -> Self {
        Self::with_replicas(value.clone(), value)
    }

    /// Stages one update into the inactive replica, publishes it, and waits for quiescence.
    ///
    /// # Errors
    ///
    /// Returns one honest synchronization error when the writer mutex cannot be acquired.
    pub fn update<R>(&self, update: impl FnOnce(&mut T) -> R) -> Result<R, SyncError> {
        let _guard = self.writer.lock()?;
        let active_side = self.active_side.load(Ordering::Acquire);
        let inactive_side = alternate_side(active_side);

        // SAFETY: the writer mutex serializes exclusive mutation of the inactive replica.
        let (active, inactive) = unsafe {
            match inactive_side {
                LEFT_RIGHT_ACTIVE_LEFT => (&*self.right.get(), &mut *self.left.get()),
                LEFT_RIGHT_ACTIVE_RIGHT => (&*self.left.get(), &mut *self.right.get()),
                _ => unreachable!("left-right active side is always one of two replicas"),
            }
        };

        inactive.clone_from(active);
        let result = update(inactive);
        self.active_side.store(inactive_side, Ordering::Release);
        self.wait_for_quiescence();
        Ok(result)
    }
}

impl<T, const READERS: usize> LeftRight<T, READERS> {
    /// Creates a new left-right table from two explicit replicas.
    #[must_use]
    pub fn with_replicas(left: T, right: T) -> Self {
        Self {
            writer: ThinMutex::new(),
            active_side: AtomicUsize::new(LEFT_RIGHT_ACTIVE_LEFT),
            left: UnsafeCell::new(left),
            right: UnsafeCell::new(right),
            readers: [const { CachePadded::new(LeftRightReaderCounter::new()) }; READERS],
        }
    }

    /// Returns the number of dedicated reader slots compiled into this table.
    #[must_use]
    pub const fn reader_slots(&self) -> usize {
        READERS
    }

    /// Acquires one read slot and returns a guard into the currently published replica.
    ///
    /// Each slot is exclusive while its guard is alive. Reusing the same slot concurrently is
    /// rejected as `Busy`, which is exactly the kind of embarrassing caller bug this primitive
    /// should force into the light.
    ///
    /// # Errors
    ///
    /// Returns [`SyncError::invalid()`] when `slot` is out of range or [`SyncError::busy()`]
    /// when that slot is already live.
    pub fn read(&self, slot: usize) -> Result<LeftRightReadGuard<'_, T>, SyncError> {
        let counter = self.readers.get(slot).ok_or_else(SyncError::invalid)?;
        counter.begin()?;
        let active_side = self.active_side.load(Ordering::Acquire);
        let ptr = self.replica_ptr(active_side);
        Ok(LeftRightReadGuard { ptr, counter })
    }

    /// Replaces the inactive replica wholesale, publishes it, and waits for quiescence.
    ///
    /// # Errors
    ///
    /// Returns one honest synchronization error when the writer mutex cannot be acquired.
    pub fn replace(&self, value: T) -> Result<(), SyncError> {
        let _guard = self.writer.lock()?;
        let inactive_side = alternate_side(self.active_side.load(Ordering::Acquire));

        // SAFETY: the writer mutex serializes exclusive mutation of the inactive replica.
        unsafe {
            *self.replica_mut_ptr(inactive_side) = value;
        }

        self.active_side.store(inactive_side, Ordering::Release);
        self.wait_for_quiescence();
        Ok(())
    }

    #[inline]
    fn wait_for_quiescence(&self) {
        while !self.readers.iter().all(|reader| reader.is_quiescent()) {
            spin_loop();
        }
    }

    #[inline]
    fn replica_ptr(&self, side: usize) -> *const T {
        match side {
            LEFT_RIGHT_ACTIVE_LEFT => self.left.get().cast_const(),
            LEFT_RIGHT_ACTIVE_RIGHT => self.right.get().cast_const(),
            _ => unreachable!("left-right active side is always one of two replicas"),
        }
    }

    #[inline]
    fn replica_mut_ptr(&self, side: usize) -> *mut T {
        match side {
            LEFT_RIGHT_ACTIVE_LEFT => self.left.get(),
            LEFT_RIGHT_ACTIVE_RIGHT => self.right.get(),
            _ => unreachable!("left-right active side is always one of two replicas"),
        }
    }
}

/// Guard returned while a left-right reader slot is live.
#[must_use]
#[derive(Debug)]
pub struct LeftRightReadGuard<'a, T> {
    ptr: *const T,
    counter: &'a LeftRightReaderCounter,
}

impl<T> Deref for LeftRightReadGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: the read guard keeps its dedicated slot live until drop, and the writer does
        // not recycle the previously active replica until every slot is quiescent.
        unsafe { &*self.ptr }
    }
}

impl<T> Drop for LeftRightReadGuard<'_, T> {
    fn drop(&mut self) {
        self.counter.finish();
    }
}

const fn alternate_side(side: usize) -> usize {
    if side == LEFT_RIGHT_ACTIVE_LEFT {
        LEFT_RIGHT_ACTIVE_RIGHT
    } else {
        LEFT_RIGHT_ACTIVE_LEFT
    }
}

#[cfg(all(test, feature = "std", not(target_os = "none")))]
mod tests {
    use super::*;
    use crate::sync::SyncErrorKind;
    extern crate std;
    use self::std::sync::Arc;
    use self::std::sync::atomic::{AtomicBool, Ordering as StdOrdering};
    use self::std::thread;
    use self::std::time::Duration;

    #[test]
    fn left_right_reads_and_updates_current_value() {
        let table = LeftRight::<[u32; 4], 2>::new([1, 2, 3, 4]);

        {
            let view = table.read(0).expect("reader slot should be available");
            assert_eq!(view[2], 3);
        }

        table
            .update(|inactive| inactive[2] = 99)
            .expect("update should succeed");

        let view = table
            .read(1)
            .expect("second reader slot should be available");
        assert_eq!(view[2], 99);
    }

    #[test]
    fn left_right_rejects_reusing_one_live_reader_slot() {
        let table = LeftRight::<u32, 1>::new(7);
        let _guard = table.read(0).expect("first read should succeed");
        assert_eq!(
            table
                .read(0)
                .expect_err("same slot should not be reusable while live")
                .kind,
            SyncErrorKind::Busy
        );
    }

    #[test]
    fn left_right_writer_waits_for_live_reader_to_quiesce() {
        let table = Arc::new(LeftRight::<u32, 1>::new(1));
        let reader = table.read(0).expect("reader slot should be available");
        let writer_started = Arc::new(AtomicBool::new(false));
        let writer_finished = Arc::new(AtomicBool::new(false));

        let writer = {
            let table = Arc::clone(&table);
            let writer_started = Arc::clone(&writer_started);
            let writer_finished = Arc::clone(&writer_finished);
            thread::spawn(move || {
                writer_started.store(true, StdOrdering::Release);
                table
                    .update(|inactive| *inactive = 9)
                    .expect("update should succeed");
                writer_finished.store(true, StdOrdering::Release);
            })
        };

        while !writer_started.load(StdOrdering::Acquire) {
            thread::yield_now();
        }

        thread::sleep(Duration::from_millis(10));
        assert!(
            !writer_finished.load(StdOrdering::Acquire),
            "writer should still be waiting for the live reader"
        );

        drop(reader);
        writer.join().expect("writer should finish");

        let view = table.read(0).expect("reader slot should be reusable");
        assert_eq!(*view, 9);
    }
}
