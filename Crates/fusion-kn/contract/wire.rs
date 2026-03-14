//! Fixed-layout wire protocol for the mediated Fusion kernel boundary.
//!
//! The protocol is intentionally "bitflat": all messages are explicitly serialized into
//! little-endian byte buffers with fixed headers and bounded payloads. No Rust ABI, no
//! pointers, no ambient struct layout assumptions, and no allocation are allowed across the
//! boundary.

use bitflags::bitflags;

/// Four-byte protocol marker for Fusion kernel mediation.
pub const FUSION_KN_PROTOCOL_MAGIC: [u8; 4] = *b"FKN1";
/// Current protocol major version.
pub const FUSION_KN_PROTOCOL_VERSION_MAJOR: u16 = 1;
/// Current protocol minor version.
pub const FUSION_KN_PROTOCOL_VERSION_MINOR: u16 = 0;

/// Transport mechanism used to carry the Fusion kernel protocol.
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FusionKnTransportKind {
    /// Character-device byte stream such as `/dev/fusion_kn`.
    CharacterDevice = 1,
    /// Direct kernel service call or syscall-like boundary.
    KernelCall = 2,
    /// Mailbox or message-queue transport.
    Mailbox = 3,
    /// Shared-memory command ring or queue.
    SharedMemoryRing = 4,
}

impl FusionKnTransportKind {
    #[must_use]
    const fn from_u16(raw: u16) -> Option<Self> {
        match raw {
            1 => Some(Self::CharacterDevice),
            2 => Some(Self::KernelCall),
            3 => Some(Self::Mailbox),
            4 => Some(Self::SharedMemoryRing),
            _ => None,
        }
    }
}

/// Command identifier carried in each protocol header.
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FusionKnCommand {
    /// Capability and version negotiation.
    Negotiate = 1,
}

impl FusionKnCommand {
    #[must_use]
    const fn from_u16(raw: u16) -> Option<Self> {
        match raw {
            1 => Some(Self::Negotiate),
            _ => None,
        }
    }
}

/// Status code reported in protocol responses.
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FusionKnStatusCode {
    /// Operation completed successfully.
    Ok = 0,
    /// The requested operation or command is unsupported.
    Unsupported = 1,
    /// Header or framing was malformed.
    InvalidHeader = 2,
    /// Version negotiation failed.
    IncompatibleVersion = 3,
    /// Caller-supplied buffers were too small.
    BufferTooSmall = 4,
    /// The request was denied by policy or privilege.
    Denied = 5,
    /// Transport-level failure occurred inside the kernel boundary.
    TransportFault = 6,
    /// Internal fault occurred while handling the request.
    InternalFault = 7,
}

impl FusionKnStatusCode {
    #[must_use]
    const fn from_u16(raw: u16) -> Option<Self> {
        match raw {
            0 => Some(Self::Ok),
            1 => Some(Self::Unsupported),
            2 => Some(Self::InvalidHeader),
            3 => Some(Self::IncompatibleVersion),
            4 => Some(Self::BufferTooSmall),
            5 => Some(Self::Denied),
            6 => Some(Self::TransportFault),
            7 => Some(Self::InternalFault),
            _ => None,
        }
    }
}

bitflags! {
    /// Header flags describing message shape.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct FusionKnMessageFlags: u16 {
        /// Request message sent from the client toward the kernel.
        const REQUEST = 1 << 0;
        /// Response message sent from the kernel toward the client.
        const RESPONSE = 1 << 1;
        /// Message uses the fixed little-endian bitflat contract.
        const BITFLAT_LE = 1 << 2;
    }
}

bitflags! {
    /// Capability flags advertised during negotiation.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct FusionKnCapabilityFlags: u32 {
        /// Negotiation command is supported.
        const NEGOTIATION = 1 << 0;
        /// Bitflat little-endian framing is supported.
        const BITFLAT_LE = 1 << 1;
        /// Request/response sequencing with request IDs is supported.
        const REQUEST_IDS = 1 << 2;
    }
}

/// Wire-level framing or serialization failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FusionKnWireError {
    /// Provided byte slice is too small for the required encoded form.
    BufferTooSmall,
    /// Message magic or framing marker is wrong.
    InvalidMagic,
    /// Header size field is not the expected constant.
    InvalidHeaderLength,
    /// Header contains an unknown command ID.
    InvalidCommand,
    /// Header contains an unknown status code.
    InvalidStatus,
    /// Header contains an unknown transport kind.
    InvalidTransportKind,
    /// Header contains invalid flag combinations.
    InvalidFlags,
}

/// Common protocol header for every mediated message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FusionKnMessageHeader {
    /// Protocol version used by this message.
    pub version_major: u16,
    /// Protocol minor version used by this message.
    pub version_minor: u16,
    /// Transport carrying the protocol exchange.
    pub transport: FusionKnTransportKind,
    /// Command identifier.
    pub command: FusionKnCommand,
    /// Request/response flags.
    pub flags: FusionKnMessageFlags,
    /// Response status. Requests must use `Ok`.
    pub status: FusionKnStatusCode,
    /// Correlation ID for matching request and response.
    pub request_id: u32,
    /// Encoded payload length following the header.
    pub payload_bytes: u32,
}

impl FusionKnMessageHeader {
    /// Encoded byte length of the header.
    pub const ENCODED_LEN: usize = 28;
    /// Encoded byte length of the header as `u16`.
    pub const ENCODED_LEN_U16: u16 = 28;

    /// Builds a request header for the given command and payload size.
    #[must_use]
    pub const fn request(
        command: FusionKnCommand,
        transport: FusionKnTransportKind,
        request_id: u32,
        payload_bytes: u32,
    ) -> Self {
        Self {
            version_major: FUSION_KN_PROTOCOL_VERSION_MAJOR,
            version_minor: FUSION_KN_PROTOCOL_VERSION_MINOR,
            transport,
            command,
            flags: FusionKnMessageFlags::REQUEST.union(FusionKnMessageFlags::BITFLAT_LE),
            status: FusionKnStatusCode::Ok,
            request_id,
            payload_bytes,
        }
    }

    /// Encodes the header into the provided fixed-layout byte buffer.
    ///
    /// # Errors
    ///
    /// Returns an error when `dst` is smaller than [`Self::ENCODED_LEN`].
    pub fn encode_into(&self, dst: &mut [u8]) -> Result<(), FusionKnWireError> {
        if dst.len() < Self::ENCODED_LEN {
            return Err(FusionKnWireError::BufferTooSmall);
        }

        dst[..4].copy_from_slice(&FUSION_KN_PROTOCOL_MAGIC);
        write_u16(&mut dst[4..6], Self::ENCODED_LEN_U16);
        write_u16(&mut dst[6..8], self.version_major);
        write_u16(&mut dst[8..10], self.version_minor);
        write_u16(&mut dst[10..12], self.transport as u16);
        write_u16(&mut dst[12..14], self.command as u16);
        write_u16(&mut dst[14..16], self.flags.bits());
        write_u16(&mut dst[16..18], self.status as u16);
        // Reserved for future header evolution. Must remain zero in v1.
        write_u16(&mut dst[18..20], 0);
        write_u32(&mut dst[20..24], self.request_id);
        write_u32(&mut dst[24..28], self.payload_bytes);
        Ok(())
    }

    /// Decodes a header from the provided byte slice.
    ///
    /// # Errors
    ///
    /// Returns an error when the slice is too small or the framing fields are invalid.
    pub fn decode_from(src: &[u8]) -> Result<Self, FusionKnWireError> {
        if src.len() < Self::ENCODED_LEN {
            return Err(FusionKnWireError::BufferTooSmall);
        }
        if src[..4] != FUSION_KN_PROTOCOL_MAGIC {
            return Err(FusionKnWireError::InvalidMagic);
        }
        if read_u16(&src[4..6]) as usize != Self::ENCODED_LEN {
            return Err(FusionKnWireError::InvalidHeaderLength);
        }

        let transport = FusionKnTransportKind::from_u16(read_u16(&src[10..12]))
            .ok_or(FusionKnWireError::InvalidTransportKind)?;
        let command = FusionKnCommand::from_u16(read_u16(&src[12..14]))
            .ok_or(FusionKnWireError::InvalidCommand)?;
        // Framing flags remain strict in v1: unknown bits are rejected rather than retained.
        // Capability bits negotiate forward evolution; header shape does not.
        let flags = FusionKnMessageFlags::from_bits(read_u16(&src[14..16]))
            .ok_or(FusionKnWireError::InvalidFlags)?;
        let status = FusionKnStatusCode::from_u16(read_u16(&src[16..18]))
            .ok_or(FusionKnWireError::InvalidStatus)?;

        if flags.contains(FusionKnMessageFlags::REQUEST)
            == flags.contains(FusionKnMessageFlags::RESPONSE)
            || !flags.contains(FusionKnMessageFlags::BITFLAT_LE)
        {
            return Err(FusionKnWireError::InvalidFlags);
        }

        Ok(Self {
            version_major: read_u16(&src[6..8]),
            version_minor: read_u16(&src[8..10]),
            transport,
            command,
            flags,
            status,
            request_id: read_u32(&src[20..24]),
            payload_bytes: read_u32(&src[24..28]),
        })
    }
}

/// Version/capability negotiation request payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FusionKnNegotiationRequest {
    /// Lowest protocol major version the client will accept.
    pub min_version_major: u16,
    /// Lowest protocol minor version the client will accept.
    pub min_version_minor: u16,
    /// Highest protocol major version the client will accept.
    pub max_version_major: u16,
    /// Highest protocol minor version the client will accept.
    pub max_version_minor: u16,
    /// Transport kind expected by the caller.
    pub transport: FusionKnTransportKind,
    /// Capabilities expected by the caller.
    pub requested_capabilities: FusionKnCapabilityFlags,
}

impl FusionKnNegotiationRequest {
    /// Encoded byte length of the payload.
    pub const ENCODED_LEN: usize = 16;
    /// Encoded byte length of the payload as `u32`.
    pub const ENCODED_LEN_U32: u32 = 16;

    /// Builds a negotiation request for the current protocol floor and ceiling.
    #[must_use]
    pub const fn current(transport: FusionKnTransportKind) -> Self {
        Self {
            min_version_major: FUSION_KN_PROTOCOL_VERSION_MAJOR,
            min_version_minor: FUSION_KN_PROTOCOL_VERSION_MINOR,
            max_version_major: FUSION_KN_PROTOCOL_VERSION_MAJOR,
            max_version_minor: FUSION_KN_PROTOCOL_VERSION_MINOR,
            transport,
            requested_capabilities: FusionKnCapabilityFlags::NEGOTIATION
                .union(FusionKnCapabilityFlags::BITFLAT_LE)
                .union(FusionKnCapabilityFlags::REQUEST_IDS),
        }
    }

    /// Encodes the payload into the provided byte buffer.
    ///
    /// # Errors
    ///
    /// Returns an error when `dst` is smaller than [`Self::ENCODED_LEN`].
    pub fn encode_into(&self, dst: &mut [u8]) -> Result<(), FusionKnWireError> {
        if dst.len() < Self::ENCODED_LEN {
            return Err(FusionKnWireError::BufferTooSmall);
        }
        write_u16(&mut dst[0..2], self.min_version_major);
        write_u16(&mut dst[2..4], self.min_version_minor);
        write_u16(&mut dst[4..6], self.max_version_major);
        write_u16(&mut dst[6..8], self.max_version_minor);
        write_u16(&mut dst[8..10], self.transport as u16);
        write_u16(&mut dst[10..12], 0);
        write_u32(&mut dst[12..16], self.requested_capabilities.bits());
        Ok(())
    }

    /// Decodes the payload from the provided byte slice.
    ///
    /// # Errors
    ///
    /// Returns an error when the payload is too small or contains an invalid transport.
    pub fn decode_from(src: &[u8]) -> Result<Self, FusionKnWireError> {
        if src.len() < Self::ENCODED_LEN {
            return Err(FusionKnWireError::BufferTooSmall);
        }
        let transport = FusionKnTransportKind::from_u16(read_u16(&src[8..10]))
            .ok_or(FusionKnWireError::InvalidTransportKind)?;
        let requested_capabilities =
            FusionKnCapabilityFlags::from_bits_retain(read_u32(&src[12..16]));

        Ok(Self {
            min_version_major: read_u16(&src[0..2]),
            min_version_minor: read_u16(&src[2..4]),
            max_version_major: read_u16(&src[4..6]),
            max_version_minor: read_u16(&src[6..8]),
            transport,
            requested_capabilities,
        })
    }
}

/// Version/capability negotiation response payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FusionKnNegotiationResponse {
    /// Protocol major version selected by the kernel side.
    pub selected_version_major: u16,
    /// Protocol minor version selected by the kernel side.
    pub selected_version_minor: u16,
    /// Transport kind confirmed by the kernel side.
    pub transport: FusionKnTransportKind,
    /// Capabilities confirmed by the kernel side.
    pub capabilities: FusionKnCapabilityFlags,
    /// Maximum payload size accepted on this transport.
    pub max_payload_bytes: u32,
}

impl FusionKnNegotiationResponse {
    /// Encoded byte length of the payload.
    pub const ENCODED_LEN: usize = 16;
    /// Encoded byte length of the payload as `u32`.
    pub const ENCODED_LEN_U32: u32 = 16;

    /// Encodes the payload into the provided byte buffer.
    ///
    /// # Errors
    ///
    /// Returns an error when `dst` is smaller than [`Self::ENCODED_LEN`].
    pub fn encode_into(&self, dst: &mut [u8]) -> Result<(), FusionKnWireError> {
        if dst.len() < Self::ENCODED_LEN {
            return Err(FusionKnWireError::BufferTooSmall);
        }
        write_u16(&mut dst[0..2], self.selected_version_major);
        write_u16(&mut dst[2..4], self.selected_version_minor);
        write_u16(&mut dst[4..6], self.transport as u16);
        write_u16(&mut dst[6..8], 0);
        write_u32(&mut dst[8..12], self.capabilities.bits());
        write_u32(&mut dst[12..16], self.max_payload_bytes);
        Ok(())
    }

    /// Decodes the payload from the provided byte slice.
    ///
    /// # Errors
    ///
    /// Returns an error when the payload is too small or contains an invalid transport.
    pub fn decode_from(src: &[u8]) -> Result<Self, FusionKnWireError> {
        if src.len() < Self::ENCODED_LEN {
            return Err(FusionKnWireError::BufferTooSmall);
        }
        let transport = FusionKnTransportKind::from_u16(read_u16(&src[4..6]))
            .ok_or(FusionKnWireError::InvalidTransportKind)?;

        Ok(Self {
            selected_version_major: read_u16(&src[0..2]),
            selected_version_minor: read_u16(&src[2..4]),
            transport,
            capabilities: FusionKnCapabilityFlags::from_bits_retain(read_u32(&src[8..12])),
            max_payload_bytes: read_u32(&src[12..16]),
        })
    }
}

const fn write_u16(dst: &mut [u8], value: u16) {
    let bytes = value.to_le_bytes();
    dst[0] = bytes[0];
    dst[1] = bytes[1];
}

const fn write_u32(dst: &mut [u8], value: u32) {
    let bytes = value.to_le_bytes();
    dst[0] = bytes[0];
    dst[1] = bytes[1];
    dst[2] = bytes[2];
    dst[3] = bytes[3];
}

fn read_u16(src: &[u8]) -> u16 {
    u16::from_le_bytes([src[0], src[1]])
}

fn read_u32(src: &[u8]) -> u32 {
    u32::from_le_bytes([src[0], src[1], src[2], src[3]])
}
