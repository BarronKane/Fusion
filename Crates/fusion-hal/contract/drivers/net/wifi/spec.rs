//! Canonical Wi-Fi spec-layer frame vocabulary.

#[path = "spec/control.rs"]
mod control;
#[path = "spec/data.rs"]
mod data;
#[path = "spec/event.rs"]
mod event;
#[path = "spec/mac.rs"]
mod mac;

pub use control::*;
pub use data::*;
pub use event::*;
pub use mac::*;

/// Stable canonical Wi-Fi frame family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WifiCanonicalFrameKind {
    Mac,
    Data,
    Control,
    Event,
}

/// One canonical Wi-Fi frame envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WifiCanonicalFrame<'a> {
    Mac(WifiMacFrame<'a>),
    Data(WifiDataFrame<'a>),
    Control(WifiControlFrame<'a>),
    Event(WifiEventFrame<'a>),
}

impl<'a> WifiCanonicalFrame<'a> {
    /// Returns the active canonical Wi-Fi frame family.
    #[must_use]
    pub const fn kind(self) -> WifiCanonicalFrameKind {
        match self {
            Self::Mac(_) => WifiCanonicalFrameKind::Mac,
            Self::Data(_) => WifiCanonicalFrameKind::Data,
            Self::Control(_) => WifiCanonicalFrameKind::Control,
            Self::Event(_) => WifiCanonicalFrameKind::Event,
        }
    }
}

impl<'a> From<WifiMacFrame<'a>> for WifiCanonicalFrame<'a> {
    fn from(value: WifiMacFrame<'a>) -> Self {
        Self::Mac(value)
    }
}

impl<'a> From<WifiDataFrame<'a>> for WifiCanonicalFrame<'a> {
    fn from(value: WifiDataFrame<'a>) -> Self {
        Self::Data(value)
    }
}

impl<'a> From<WifiControlFrame<'a>> for WifiCanonicalFrame<'a> {
    fn from(value: WifiControlFrame<'a>) -> Self {
        Self::Control(value)
    }
}

impl<'a> From<WifiEventFrame<'a>> for WifiCanonicalFrame<'a> {
    fn from(value: WifiEventFrame<'a>) -> Self {
        Self::Event(value)
    }
}
