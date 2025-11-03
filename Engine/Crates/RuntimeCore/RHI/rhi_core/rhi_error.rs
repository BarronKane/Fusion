use core::{
    error::Error,
    fmt,
    fmt::Debug,
};

pub type Result<T> = core::result::Result<T, RHIError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RHIErrorEnum {
    InitializationError
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RHIError {
    pub rhi: &'static str,
    pub kind: RHIErrorEnum,
    pub message: &'static str
}

impl RHIError {
    pub const fn new(rhi: &'static str, kind: RHIErrorEnum, message: &'static str) -> Self {
        Self { rhi, kind, message }
    }
}

impl Error for RHIError {}

impl fmt::Display for RHIError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} -|- {:?}: {}", self.rhi, self.kind, self.message)
    }
}
