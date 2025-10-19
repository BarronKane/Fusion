use core::{
    error::Error,
    fmt,
    fmt::Debug,
};

pub type Result<'e, T> = core::result::Result<T, RHIError<'e>>;

#[derive(Debug, Clone)]
pub enum RHIErrorEnum {
    InitializationError
}

#[derive(Debug, Clone)]
pub struct RHIError<'e> {
    pub rhi: &'e str,
    pub kind: &'e RHIErrorEnum,
    pub message: &'e str
}

impl<'e> Error for RHIError<'e> {}

impl<'e> fmt::Display for RHIError<'e> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        return write!(f, "{} -|- {:?}: {}", self.rhi, self.kind, self.message);
    }
}
