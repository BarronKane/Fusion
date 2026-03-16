//! Public hosted-fiber support export for the selected platform backend.
//!
//! This surface exists for runtime machinery that is lower than `fusion-std` scheduling policy
//! but higher than raw context switching: signal-stack installation for hosted elastic stacks,
//! process-global fault-handler installation, and user-space wake signals that can be registered
//! with readiness pollers.

/// Concrete hosted-fiber support vocabulary shared across supported backends.
pub use super::fiber_common::{
    FiberHostError, FiberHostErrorKind, FiberHostSupport, PlatformElasticFaultHandler,
    PlatformWakeToken,
};
/// Concrete hosted-fiber support types and constructor for the selected platform.
pub use super::platform::fiber::{
    PlatformFiberHost, PlatformFiberSignalStack, PlatformFiberWakeSignal, system_fiber_host,
};
