//! Counting semaphore wrapper built on top of the selected fusion-pal raw semaphore backend.

use core::time::Duration;

use fusion_pal::sys::sync::{
    PlatformSemaphore,
    RawSemaphoreContract,
    SemaphoreSupport,
};

use super::SyncError;

/// Counting semaphore with no hidden allocation and no poisoning semantics.
#[derive(Debug)]
pub struct Semaphore {
    raw: PlatformSemaphore,
}

impl Semaphore {
    /// Creates a new semaphore with the supplied initial and maximum permit counts.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend rejects the requested semaphore shape or does not
    /// support semaphores honestly on the current platform.
    pub fn new(initial: u32, max: u32) -> Result<Self, SyncError> {
        match PlatformSemaphore::new(initial, max) {
            Ok(raw) => Ok(Self { raw }),
            Err(error) => Err(error),
        }
    }

    /// Reports the support surface of the selected raw semaphore backend.
    #[must_use]
    pub fn support(&self) -> SemaphoreSupport {
        self.raw.support()
    }

    /// Blocks until one permit can be acquired.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot complete the acquisition honestly.
    pub fn acquire(&self) -> Result<(), SyncError> {
        self.raw.acquire()
    }

    /// Attempts to acquire one permit without blocking.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot evaluate the request honestly.
    pub fn try_acquire(&self) -> Result<bool, SyncError> {
        self.raw.try_acquire()
    }

    /// Attempts to acquire one permit within a relative timeout.
    ///
    /// # Errors
    ///
    /// Returns an error if timed acquisition is unsupported or the backend cannot evaluate
    /// the request honestly.
    pub fn acquire_for(&self, timeout: Duration) -> Result<bool, SyncError> {
        self.raw.acquire_for(timeout)
    }

    /// Releases `permits` back to the semaphore.
    ///
    /// # Errors
    ///
    /// Returns an error if the release would violate the semaphore's permit bounds.
    pub fn release(&self, permits: u32) -> Result<(), SyncError> {
        self.raw.release(permits)
    }

    /// Returns the maximum number of permits the semaphore can represent.
    #[must_use]
    pub fn max_permits(&self) -> u32 {
        self.raw.max_permits()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fusion_pal::sys::sync::SyncImplementationKind;

    #[test]
    fn semaphore_tracks_permits_or_reports_unsupported() {
        let semaphore = Semaphore::new(1, 2);

        match semaphore {
            Ok(semaphore) => {
                assert_ne!(
                    semaphore.support().implementation,
                    SyncImplementationKind::Unsupported
                );
                assert!(
                    semaphore
                        .try_acquire()
                        .expect("first acquire should evaluate")
                );
                assert!(
                    !semaphore
                        .try_acquire()
                        .expect("second acquire should evaluate")
                );
                semaphore
                    .release(1)
                    .expect("release should restore a permit");
                assert!(semaphore.try_acquire().expect("reacquire should evaluate"));
            }
            Err(error) => assert_eq!(error, SyncError::unsupported()),
        }
    }
}
