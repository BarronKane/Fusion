use core::fmt;

mod sealed {
    pub trait Sealed {}
}

/// Typestate for allocator-backed storage that tears down when the owner drops.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Mortal;

/// Typestate for allocator-backed storage that intentionally remains alive until process exit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Immortal;

impl sealed::Sealed for Mortal {}
impl sealed::Sealed for Immortal {}

/// Lifetime policy for allocator subsystem instances.
pub trait LifetimePolicy: sealed::Sealed + Copy + fmt::Debug + 'static {
    /// Whether dropping the wrapper intentionally leaves the backing alive until process exit.
    const IMMORTAL: bool;
}

impl LifetimePolicy for Mortal {
    const IMMORTAL: bool = false;
}

impl LifetimePolicy for Immortal {
    const IMMORTAL: bool = true;
}
