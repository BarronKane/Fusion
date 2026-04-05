//! Unsupported protocol placeholder.

use super::{
    ProtocolContract,
    ProtocolDescriptor,
};

/// Unsupported protocol descriptor placeholder.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedProtocol;

impl ProtocolContract for UnsupportedProtocol {
    type Message = ();

    const DESCRIPTOR: ProtocolDescriptor = ProtocolDescriptor::unsupported();
}
