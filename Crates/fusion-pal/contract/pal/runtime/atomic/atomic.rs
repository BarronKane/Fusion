//! Backend-neutral atomic vocabulary and low-level fusion-pal contracts.
//!
//! This lane is intentionally narrower than Rust's full atomic type zoo. Fusion needs one honest
//! runtime atomic substrate first, not a vanity wrapper over every integer width the language can
//! spell. The first explicit contract is therefore one 32-bit atomic word surface plus truthful
//! wait/wake capability reporting for platforms that can support it.

mod caps;
mod error;
mod unsupported;
mod word;

pub use caps::*;
pub use error::*;
pub use unsupported::*;
pub use word::*;

/// Backend-selected atomic support surface.
pub trait AtomicBaseContract {
    /// Concrete 32-bit atomic word handle returned by this backend.
    type Word32: AtomicWord32Contract;

    /// Reports the atomic surfaces and semantics this backend can support honestly.
    fn support(&self) -> AtomicSupport;

    /// Creates one backend-selected 32-bit atomic word.
    ///
    /// # Errors
    ///
    /// Returns an error when the backend cannot honestly realize this atomic word surface.
    fn new_word32(&self, initial: u32) -> Result<Self::Word32, AtomicError>;
}
