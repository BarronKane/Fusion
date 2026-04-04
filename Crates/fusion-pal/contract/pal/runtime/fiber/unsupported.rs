//! Backend-neutral unsupported hosted-fiber helper implementation.

use super::{
    FiberHostError,
    FiberHostSupport,
    PlatformElasticFaultHandler,
    PlatformWakeToken,
};

/// Unsupported hosted-fiber helper provider.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedFiberHost;

/// Unsupported alternate-signal-stack guard.
#[derive(Debug)]
pub struct UnsupportedFiberSignalStack;

/// Unsupported wake signal compatible with readiness registration.
#[derive(Debug)]
pub struct UnsupportedFiberWakeSignal;

impl UnsupportedFiberHost {
    /// Creates a new unsupported hosted-fiber helper provider.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Returns the truthful helper support surface.
    #[must_use]
    pub const fn support(&self) -> FiberHostSupport {
        FiberHostSupport {
            elastic_stack_faults: false,
            signal_stack: false,
            wake_signal: false,
        }
    }

    /// Installs a platform fault handler for elastic stack promotion.
    ///
    /// # Errors
    ///
    /// Always returns `Unsupported` on this backend.
    pub fn ensure_elastic_fault_handler(
        &self,
        _handler: PlatformElasticFaultHandler,
    ) -> Result<(), FiberHostError> {
        Err(FiberHostError::unsupported())
    }

    /// Promotes one detector page to read/write access.
    ///
    /// # Errors
    ///
    /// Always returns `Unsupported` on this backend.
    pub const fn promote_elastic_page(
        &self,
        _base: usize,
        _len: usize,
    ) -> Result<(), FiberHostError> {
        Err(FiberHostError::unsupported())
    }

    /// Installs one alternate signal stack for the current carrier thread.
    ///
    /// # Errors
    ///
    /// Always returns `Unsupported` on this backend.
    pub const fn install_signal_stack(
        &self,
    ) -> Result<UnsupportedFiberSignalStack, FiberHostError> {
        Err(FiberHostError::unsupported())
    }

    /// Creates one wake signal that can be registered with a readiness poller.
    ///
    /// # Errors
    ///
    /// Always returns `Unsupported` on this backend.
    pub const fn create_wake_signal(&self) -> Result<UnsupportedFiberWakeSignal, FiberHostError> {
        Err(FiberHostError::unsupported())
    }

    /// Signals one wake token from a fault or scheduler path.
    ///
    /// # Errors
    ///
    /// Always returns `Unsupported` when a valid token is supplied on this backend.
    pub const fn notify_wake_token(&self, token: PlatformWakeToken) -> Result<(), FiberHostError> {
        if token.is_valid() {
            Err(FiberHostError::unsupported())
        } else {
            Ok(())
        }
    }
}

impl UnsupportedFiberWakeSignal {
    /// Returns the source handle used to register this signal with a readiness poller.
    ///
    /// # Errors
    ///
    /// Always returns `Unsupported` on this backend.
    pub const fn source_handle(&self) -> Result<usize, FiberHostError> {
        Err(FiberHostError::unsupported())
    }

    /// Returns the wake token associated with this signal.
    #[must_use]
    pub const fn token(&self) -> PlatformWakeToken {
        PlatformWakeToken::invalid()
    }

    /// Signals the wake source.
    ///
    /// # Errors
    ///
    /// Always returns `Unsupported` on this backend.
    pub const fn signal(&self) -> Result<(), FiberHostError> {
        Err(FiberHostError::unsupported())
    }

    /// Drains the wake source after one readiness notification.
    ///
    /// # Errors
    ///
    /// Always returns `Unsupported` on this backend.
    pub const fn drain(&self) -> Result<(), FiberHostError> {
        Err(FiberHostError::unsupported())
    }
}
