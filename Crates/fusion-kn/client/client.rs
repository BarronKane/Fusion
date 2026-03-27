//! No-alloc client helpers for the mediated Fusion kernel boundary.
//!
//! The client side remains transport-neutral. `fusion-pal` or other consumers provide the
//! actual transport implementation; this module owns the bitflat negotiation and response
//! validation rules.

use crate::contract::wire::{
    FusionKnCapabilityFlags,
    FusionKnCommand,
    FusionKnMessageFlags,
    FusionKnMessageHeader,
    FusionKnNegotiationRequest,
    FusionKnNegotiationResponse,
    FusionKnStatusCode,
    FusionKnTransportKind,
    FusionKnWireError,
};

/// Abstract transport for sending and receiving mediated Fusion kernel messages.
pub trait FusionKnTransport {
    /// Concrete transport-level error reported by the implementation.
    type Error;

    /// Returns the transport kind used by this client.
    fn transport_kind(&self) -> FusionKnTransportKind;

    /// Performs one request/response exchange.
    ///
    /// The request buffer contains a full encoded message. The transport must fill
    /// `response` with exactly one full encoded response and return the number of bytes
    /// written into it.
    ///
    /// # Errors
    ///
    /// Returns the transport's concrete error type when the exchange cannot be completed.
    fn transact(&mut self, request: &[u8], response: &mut [u8]) -> Result<usize, Self::Error>;
}

/// Negotiated session parameters returned by the kernel boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FusionKnNegotiatedSession {
    /// Protocol major version selected by the peer.
    pub version_major: u16,
    /// Protocol minor version selected by the peer.
    pub version_minor: u16,
    /// Transport kind confirmed by the peer.
    pub transport: FusionKnTransportKind,
    /// Capability set confirmed by the peer.
    pub capabilities: FusionKnCapabilityFlags,
    /// Maximum payload length accepted on this session.
    pub max_payload_bytes: u32,
}

/// Client-side protocol or transport failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FusionKnClientError<E> {
    /// Transport implementation reported a failure.
    Transport(E),
    /// Wire framing or bitflat decoding failed.
    Wire(FusionKnWireError),
    /// The peer returned a non-success status.
    Status(FusionKnStatusCode),
    /// The response transport does not match the client transport.
    TransportMismatch,
    /// The response request ID did not match the current request.
    RequestIdMismatch,
    /// The response payload is larger than the caller-supplied response buffer.
    ResponseTooLarge,
    /// The peer responded with an incompatible version.
    IncompatibleVersion,
}

impl<E> From<FusionKnWireError> for FusionKnClientError<E> {
    fn from(value: FusionKnWireError) -> Self {
        Self::Wire(value)
    }
}

/// Stateful no-alloc client for the mediated Fusion kernel protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FusionKnClient<T> {
    transport: T,
    next_request_id: u32,
}

impl<T> FusionKnClient<T> {
    /// Creates a client over the provided transport.
    #[must_use]
    pub const fn new(transport: T) -> Self {
        Self {
            transport,
            next_request_id: 1,
        }
    }

    /// Returns the inner transport by value.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.transport
    }
}

impl<T> FusionKnClient<T>
where
    T: FusionKnTransport,
{
    /// Performs version and capability negotiation with the kernel peer.
    ///
    /// # Errors
    ///
    /// Returns an error when transport exchange fails, framing is invalid, or the peer does
    /// not accept the current protocol version and transport.
    pub fn negotiate(
        &mut self,
    ) -> Result<FusionKnNegotiatedSession, FusionKnClientError<T::Error>> {
        let request_id = self.allocate_request_id();
        let request_payload = FusionKnNegotiationRequest::current(self.transport.transport_kind());
        let request_header = FusionKnMessageHeader::request(
            FusionKnCommand::Negotiate,
            self.transport.transport_kind(),
            request_id,
            FusionKnNegotiationRequest::ENCODED_LEN_U32,
        );

        let mut request =
            [0_u8; FusionKnMessageHeader::ENCODED_LEN + FusionKnNegotiationRequest::ENCODED_LEN];
        request_header.encode_into(&mut request[..FusionKnMessageHeader::ENCODED_LEN])?;
        request_payload.encode_into(&mut request[FusionKnMessageHeader::ENCODED_LEN..])?;

        let mut response =
            [0_u8; FusionKnMessageHeader::ENCODED_LEN + FusionKnNegotiationResponse::ENCODED_LEN];
        let response_bytes = self
            .transport
            .transact(&request, &mut response)
            .map_err(FusionKnClientError::Transport)?;
        if response_bytes < FusionKnMessageHeader::ENCODED_LEN {
            return Err(FusionKnClientError::Wire(FusionKnWireError::BufferTooSmall));
        }

        let header =
            FusionKnMessageHeader::decode_from(&response[..FusionKnMessageHeader::ENCODED_LEN])?;
        if !header.flags.contains(FusionKnMessageFlags::RESPONSE) {
            return Err(FusionKnClientError::Wire(FusionKnWireError::InvalidFlags));
        }
        if header.request_id != request_id {
            return Err(FusionKnClientError::RequestIdMismatch);
        }
        if header.transport != self.transport.transport_kind() {
            return Err(FusionKnClientError::TransportMismatch);
        }
        if header.status != FusionKnStatusCode::Ok {
            return Err(FusionKnClientError::Status(header.status));
        }

        let required = FusionKnMessageHeader::ENCODED_LEN
            + usize::try_from(header.payload_bytes)
                .map_err(|_| FusionKnClientError::ResponseTooLarge)?;
        if required > response.len() || response_bytes < required {
            return Err(FusionKnClientError::ResponseTooLarge);
        }

        let negotiation = FusionKnNegotiationResponse::decode_from(
            &response[FusionKnMessageHeader::ENCODED_LEN..required],
        )?;
        if negotiation.selected_version_major != request_payload.max_version_major
            || negotiation.selected_version_minor < request_payload.min_version_minor
            || negotiation.transport != self.transport.transport_kind()
        {
            return Err(FusionKnClientError::IncompatibleVersion);
        }

        Ok(FusionKnNegotiatedSession {
            version_major: negotiation.selected_version_major,
            version_minor: negotiation.selected_version_minor,
            transport: negotiation.transport,
            capabilities: negotiation.capabilities,
            max_payload_bytes: negotiation.max_payload_bytes,
        })
    }

    const fn allocate_request_id(&mut self) -> u32 {
        let current = self.next_request_id;
        self.next_request_id = if current == u32::MAX { 1 } else { current + 1 };
        current
    }
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
    use super::*;
    use crate::contract::wire::{
        FUSION_KN_PROTOCOL_VERSION_MAJOR,
        FUSION_KN_PROTOCOL_VERSION_MINOR,
    };

    extern crate std;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    struct FakeTransport;

    impl FusionKnTransport for FakeTransport {
        type Error = ();

        fn transport_kind(&self) -> FusionKnTransportKind {
            FusionKnTransportKind::CharacterDevice
        }

        fn transact(&mut self, request: &[u8], response: &mut [u8]) -> Result<usize, Self::Error> {
            let request_header =
                FusionKnMessageHeader::decode_from(&request[..FusionKnMessageHeader::ENCODED_LEN])
                    .expect("request header should decode");
            let negotiation = FusionKnNegotiationResponse {
                selected_version_major: FUSION_KN_PROTOCOL_VERSION_MAJOR,
                selected_version_minor: FUSION_KN_PROTOCOL_VERSION_MINOR,
                transport: FusionKnTransportKind::CharacterDevice,
                capabilities: FusionKnCapabilityFlags::NEGOTIATION
                    | FusionKnCapabilityFlags::BITFLAT_LE
                    | FusionKnCapabilityFlags::REQUEST_IDS,
                max_payload_bytes: 1024,
            };
            let header = FusionKnMessageHeader {
                version_major: FUSION_KN_PROTOCOL_VERSION_MAJOR,
                version_minor: FUSION_KN_PROTOCOL_VERSION_MINOR,
                transport: FusionKnTransportKind::CharacterDevice,
                command: FusionKnCommand::Negotiate,
                flags: FusionKnMessageFlags::RESPONSE | FusionKnMessageFlags::BITFLAT_LE,
                status: FusionKnStatusCode::Ok,
                request_id: request_header.request_id,
                payload_bytes: FusionKnNegotiationResponse::ENCODED_LEN_U32,
            };

            header
                .encode_into(&mut response[..FusionKnMessageHeader::ENCODED_LEN])
                .expect("response header should encode");
            negotiation
                .encode_into(
                    &mut response[FusionKnMessageHeader::ENCODED_LEN
                        ..FusionKnMessageHeader::ENCODED_LEN
                            + FusionKnNegotiationResponse::ENCODED_LEN],
                )
                .expect("response payload should encode");

            Ok(FusionKnMessageHeader::ENCODED_LEN + FusionKnNegotiationResponse::ENCODED_LEN)
        }
    }

    #[test]
    fn client_negotiates_current_protocol() {
        let mut client = FusionKnClient::new(FakeTransport);
        let session = client.negotiate().expect("negotiation should succeed");

        assert_eq!(session.version_major, FUSION_KN_PROTOCOL_VERSION_MAJOR);
        assert_eq!(session.version_minor, FUSION_KN_PROTOCOL_VERSION_MINOR);
        assert_eq!(session.transport, FusionKnTransportKind::CharacterDevice);
        assert_eq!(session.max_payload_bytes, 1024);
        assert!(
            session
                .capabilities
                .contains(FusionKnCapabilityFlags::NEGOTIATION)
        );
    }
}
