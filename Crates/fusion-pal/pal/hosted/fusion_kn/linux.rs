//! Linux character-device transport for the mediated Fusion kernel backend.
//!
//! The transport is intentionally conservative:
//! - request bytes are fully written before any response is read
//! - response framing is validated from the fixed bitflat header
//! - oversized payloads are drained before reporting failure so the stream does not remain
//!   poisoned for the next exchange

use core::ffi::c_int;
use core::fmt;

use fusion_kn::client::FusionKnTransport;
use fusion_kn::contract::wire::{
    FusionKnMessageHeader,
    FusionKnTransportKind,
    FusionKnWireError,
};

/// Default Linux device path for the mediated Fusion kernel transport.
pub const DEFAULT_DEVICE_PATH: &[u8] = b"/dev/fusion_kn\0";

/// Linux transport error for mediated character-device exchanges.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinuxFusionKnTransportError {
    /// Caller passed an invalid or negative file descriptor.
    InvalidFileDescriptor,
    /// Caller passed a path that was not explicitly nul-terminated.
    InvalidPath,
    /// Device open failed with the contained errno.
    OpenFailed(i32),
    /// Device write failed with the contained errno.
    WriteFailed(i32),
    /// Device read failed with the contained errno.
    ReadFailed(i32),
    /// Stream was closed before the expected number of bytes arrived.
    UnexpectedEndOfStream,
    /// Response framing failed before the message could be completed.
    Wire(FusionKnWireError),
}

impl fmt::Display for LinuxFusionKnTransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFileDescriptor => f.write_str("invalid file descriptor"),
            Self::InvalidPath => f.write_str("device path must be nul-terminated"),
            Self::OpenFailed(errno) => write!(f, "device open failed with errno {errno}"),
            Self::WriteFailed(errno) => write!(f, "device write failed with errno {errno}"),
            Self::ReadFailed(errno) => write!(f, "device read failed with errno {errno}"),
            Self::UnexpectedEndOfStream => {
                f.write_str("device stream ended before a full message arrived")
            }
            Self::Wire(error) => write!(f, "response framing failed: {error:?}"),
        }
    }
}

/// Character-device transport backed by a Linux file descriptor.
#[derive(Debug)]
pub struct LinuxFusionKnCharacterDevice {
    fd: c_int,
}

impl LinuxFusionKnCharacterDevice {
    /// Opens the default `/dev/fusion_kn` device.
    ///
    /// # Errors
    ///
    /// Returns an error when the device cannot be opened.
    pub fn open_default() -> Result<Self, LinuxFusionKnTransportError> {
        Self::open_path(DEFAULT_DEVICE_PATH)
    }

    /// Opens the device at the provided nul-terminated path.
    ///
    /// The provided path must end with a trailing `\0`, because this method forwards the
    /// slice directly to `libc::open`.
    ///
    /// # Errors
    ///
    /// Returns an error when the path is not nul-terminated or the device cannot be opened.
    pub fn open_path(path: &[u8]) -> Result<Self, LinuxFusionKnTransportError> {
        if path.last().copied() != Some(0) {
            return Err(LinuxFusionKnTransportError::InvalidPath);
        }

        let fd = unsafe { libc::open(path.as_ptr().cast(), libc::O_RDWR | libc::O_CLOEXEC) };
        if fd < 0 {
            return Err(LinuxFusionKnTransportError::OpenFailed(last_errno()));
        }
        Ok(Self { fd })
    }

    /// Creates a transport from an already-open file descriptor.
    ///
    /// # Errors
    ///
    /// Returns an error when `fd` is negative.
    pub const fn from_raw_fd(fd: c_int) -> Result<Self, LinuxFusionKnTransportError> {
        if fd < 0 {
            return Err(LinuxFusionKnTransportError::InvalidFileDescriptor);
        }
        Ok(Self { fd })
    }
}

impl FusionKnTransport for LinuxFusionKnCharacterDevice {
    type Error = LinuxFusionKnTransportError;

    fn transport_kind(&self) -> FusionKnTransportKind {
        FusionKnTransportKind::CharacterDevice
    }

    fn transact(&mut self, request: &[u8], response: &mut [u8]) -> Result<usize, Self::Error> {
        write_all(self.fd, request)?;

        if response.len() < FusionKnMessageHeader::ENCODED_LEN {
            return Err(LinuxFusionKnTransportError::Wire(
                FusionKnWireError::BufferTooSmall,
            ));
        }

        read_exact(self.fd, &mut response[..FusionKnMessageHeader::ENCODED_LEN])?;
        let header =
            FusionKnMessageHeader::decode_from(&response[..FusionKnMessageHeader::ENCODED_LEN])
                .map_err(LinuxFusionKnTransportError::Wire)?;
        let payload_bytes = usize::try_from(header.payload_bytes)
            .map_err(|_| LinuxFusionKnTransportError::Wire(FusionKnWireError::BufferTooSmall))?;
        let total = FusionKnMessageHeader::ENCODED_LEN + payload_bytes;

        if total > response.len() {
            discard_exact(self.fd, payload_bytes)?;
            return Err(LinuxFusionKnTransportError::Wire(
                FusionKnWireError::BufferTooSmall,
            ));
        }

        read_exact(
            self.fd,
            &mut response[FusionKnMessageHeader::ENCODED_LEN..total],
        )?;
        Ok(total)
    }
}

impl Drop for LinuxFusionKnCharacterDevice {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.fd);
        }
    }
}

fn write_all(fd: c_int, bytes: &[u8]) -> Result<(), LinuxFusionKnTransportError> {
    let mut written = 0;
    while written < bytes.len() {
        let rc =
            unsafe { libc::write(fd, bytes[written..].as_ptr().cast(), bytes.len() - written) };
        if rc < 0 {
            let errno = last_errno();
            if errno == libc::EINTR {
                continue;
            }
            return Err(LinuxFusionKnTransportError::WriteFailed(errno));
        }
        if rc == 0 {
            return Err(LinuxFusionKnTransportError::UnexpectedEndOfStream);
        }
        written += rc.cast_unsigned();
    }
    Ok(())
}

fn read_exact(fd: c_int, dst: &mut [u8]) -> Result<(), LinuxFusionKnTransportError> {
    let mut read = 0;
    while read < dst.len() {
        let rc = unsafe { libc::read(fd, dst[read..].as_mut_ptr().cast(), dst.len() - read) };
        if rc < 0 {
            let errno = last_errno();
            if errno == libc::EINTR {
                continue;
            }
            return Err(LinuxFusionKnTransportError::ReadFailed(errno));
        }
        if rc == 0 {
            return Err(LinuxFusionKnTransportError::UnexpectedEndOfStream);
        }
        read += rc.cast_unsigned();
    }
    Ok(())
}

fn discard_exact(fd: c_int, bytes: usize) -> Result<(), LinuxFusionKnTransportError> {
    let mut remaining = bytes;
    let mut scratch = [0_u8; 256];
    while remaining != 0 {
        let chunk = remaining.min(scratch.len());
        read_exact(fd, &mut scratch[..chunk])?;
        remaining -= chunk;
    }
    Ok(())
}

fn last_errno() -> i32 {
    unsafe { *libc::__errno_location() }
}
