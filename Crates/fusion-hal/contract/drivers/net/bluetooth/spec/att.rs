//! Canonical Bluetooth ATT spec PDUs.

/// Canonical ATT opcode vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BluetoothAttOpcode {
    ErrorResponse,
    ExchangeMtuRequest,
    ExchangeMtuResponse,
    FindInformationRequest,
    FindInformationResponse,
    FindByTypeValueRequest,
    FindByTypeValueResponse,
    ReadByTypeRequest,
    ReadByTypeResponse,
    ReadRequest,
    ReadResponse,
    ReadBlobRequest,
    ReadBlobResponse,
    ReadMultipleRequest,
    ReadMultipleResponse,
    ReadByGroupTypeRequest,
    ReadByGroupTypeResponse,
    WriteRequest,
    WriteResponse,
    WriteCommand,
    PrepareWriteRequest,
    PrepareWriteResponse,
    ExecuteWriteRequest,
    ExecuteWriteResponse,
    HandleValueNotification,
    HandleValueIndication,
    HandleValueConfirmation,
    SignedWriteCommand,
    Unknown(u8),
}

impl BluetoothAttOpcode {
    /// Returns the canonical opcode byte.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        match self {
            Self::ErrorResponse => 0x01,
            Self::ExchangeMtuRequest => 0x02,
            Self::ExchangeMtuResponse => 0x03,
            Self::FindInformationRequest => 0x04,
            Self::FindInformationResponse => 0x05,
            Self::FindByTypeValueRequest => 0x06,
            Self::FindByTypeValueResponse => 0x07,
            Self::ReadByTypeRequest => 0x08,
            Self::ReadByTypeResponse => 0x09,
            Self::ReadRequest => 0x0A,
            Self::ReadResponse => 0x0B,
            Self::ReadBlobRequest => 0x0C,
            Self::ReadBlobResponse => 0x0D,
            Self::ReadMultipleRequest => 0x0E,
            Self::ReadMultipleResponse => 0x0F,
            Self::ReadByGroupTypeRequest => 0x10,
            Self::ReadByGroupTypeResponse => 0x11,
            Self::WriteRequest => 0x12,
            Self::WriteResponse => 0x13,
            Self::WriteCommand => 0x52,
            Self::PrepareWriteRequest => 0x16,
            Self::PrepareWriteResponse => 0x17,
            Self::ExecuteWriteRequest => 0x18,
            Self::ExecuteWriteResponse => 0x19,
            Self::HandleValueNotification => 0x1B,
            Self::HandleValueIndication => 0x1D,
            Self::HandleValueConfirmation => 0x1E,
            Self::SignedWriteCommand => 0xD2,
            Self::Unknown(value) => value,
        }
    }

    /// Parses one opcode byte into canonical ATT vocabulary.
    #[must_use]
    pub const fn from_u8(value: u8) -> Self {
        match value {
            0x01 => Self::ErrorResponse,
            0x02 => Self::ExchangeMtuRequest,
            0x03 => Self::ExchangeMtuResponse,
            0x04 => Self::FindInformationRequest,
            0x05 => Self::FindInformationResponse,
            0x06 => Self::FindByTypeValueRequest,
            0x07 => Self::FindByTypeValueResponse,
            0x08 => Self::ReadByTypeRequest,
            0x09 => Self::ReadByTypeResponse,
            0x0A => Self::ReadRequest,
            0x0B => Self::ReadResponse,
            0x0C => Self::ReadBlobRequest,
            0x0D => Self::ReadBlobResponse,
            0x0E => Self::ReadMultipleRequest,
            0x0F => Self::ReadMultipleResponse,
            0x10 => Self::ReadByGroupTypeRequest,
            0x11 => Self::ReadByGroupTypeResponse,
            0x12 => Self::WriteRequest,
            0x13 => Self::WriteResponse,
            0x16 => Self::PrepareWriteRequest,
            0x17 => Self::PrepareWriteResponse,
            0x18 => Self::ExecuteWriteRequest,
            0x19 => Self::ExecuteWriteResponse,
            0x1B => Self::HandleValueNotification,
            0x1D => Self::HandleValueIndication,
            0x1E => Self::HandleValueConfirmation,
            0x52 => Self::WriteCommand,
            0xD2 => Self::SignedWriteCommand,
            other => Self::Unknown(other),
        }
    }
}

/// One canonical ATT PDU.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BluetoothAttPdu<'a> {
    pub opcode: BluetoothAttOpcode,
    pub parameters: &'a [u8],
}
