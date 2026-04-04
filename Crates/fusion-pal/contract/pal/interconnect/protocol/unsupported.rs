//! Unsupported protocol placeholder.

use super::{
    Protocol,
    ProtocolDescriptor,
};

/// Unsupported protocol descriptor placeholder.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedProtocol;

impl Protocol for UnsupportedProtocol {
    type Message = ();

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor::unsupported();
}
