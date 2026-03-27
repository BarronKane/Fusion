use bitflags::bitflags;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GpioErrorKind {
    Unsupported,
    Invalid,
    Busy,
    ResourceExhausted,
    Platform(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpioError {
    pub kind: GpioErrorKind,
}

impl GpioError {
    pub const fn unsupported() -> Self {
        Self {
            kind: GpioErrorKind::Unsupported,
        }
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct GpioCapabilities: u32 {
        const INPUT = 1 << 0;
        const OUTPUT = 1 << 1;
        const PULL = 1 << 2;
        const DRIVE_STRENGTH = 1 << 3;
        const ALT_FUNCTION = 1 << 4;
        const INTERRUPT = 1 << 5;
    }
}

pub trait GpioContract {
    fn pin_number(&self) -> u16;
    fn capabilities(&self) -> GpioCapabilities;
}

pub trait GpioOutputContract: GpioContract {
    fn set_level(&mut self, high: bool) -> Result<(), GpioError>;
}

pub trait GpioInputContract: GpioContract {
    fn level(&self) -> Result<bool, GpioError>;
}
