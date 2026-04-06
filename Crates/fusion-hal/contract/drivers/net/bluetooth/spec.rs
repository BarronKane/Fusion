//! Canonical Bluetooth spec-layer frame vocabulary.

#[path = "spec/att.rs"]
mod att;
#[path = "spec/gap.rs"]
mod gap;
#[path = "spec/gatt.rs"]
mod gatt;
#[path = "spec/hci.rs"]
mod hci;
#[path = "spec/l2cap.rs"]
mod l2cap;
#[path = "spec/le_air.rs"]
mod le_air;

pub use att::*;
pub use gap::*;
pub use gatt::*;
pub use hci::*;
pub use l2cap::*;
pub use le_air::*;

/// Stable canonical Bluetooth frame family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BluetoothCanonicalFrameKind {
    Hci,
    L2cap,
    Att,
    Gatt,
    Gap,
    LeAir,
}

/// One canonical Bluetooth frame envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BluetoothCanonicalFrame<'a> {
    Hci(BluetoothHciFrameView<'a>),
    L2cap(BluetoothL2capFrame<'a>),
    Att(BluetoothAttPdu<'a>),
    Gatt(BluetoothGattFrame<'a>),
    Gap(BluetoothGapFrame<'a>),
    LeAir(BluetoothLeAirFrame<'a>),
}

impl<'a> BluetoothCanonicalFrame<'a> {
    /// Returns the active canonical frame family.
    #[must_use]
    pub const fn kind(self) -> BluetoothCanonicalFrameKind {
        match self {
            Self::Hci(_) => BluetoothCanonicalFrameKind::Hci,
            Self::L2cap(_) => BluetoothCanonicalFrameKind::L2cap,
            Self::Att(_) => BluetoothCanonicalFrameKind::Att,
            Self::Gatt(_) => BluetoothCanonicalFrameKind::Gatt,
            Self::Gap(_) => BluetoothCanonicalFrameKind::Gap,
            Self::LeAir(_) => BluetoothCanonicalFrameKind::LeAir,
        }
    }
}

impl<'a> From<BluetoothHciFrameView<'a>> for BluetoothCanonicalFrame<'a> {
    fn from(value: BluetoothHciFrameView<'a>) -> Self {
        Self::Hci(value)
    }
}

impl<'a> From<BluetoothL2capFrame<'a>> for BluetoothCanonicalFrame<'a> {
    fn from(value: BluetoothL2capFrame<'a>) -> Self {
        Self::L2cap(value)
    }
}

impl<'a> From<BluetoothAttPdu<'a>> for BluetoothCanonicalFrame<'a> {
    fn from(value: BluetoothAttPdu<'a>) -> Self {
        Self::Att(value)
    }
}

impl<'a> From<BluetoothGattFrame<'a>> for BluetoothCanonicalFrame<'a> {
    fn from(value: BluetoothGattFrame<'a>) -> Self {
        Self::Gatt(value)
    }
}

impl<'a> From<BluetoothGapFrame<'a>> for BluetoothCanonicalFrame<'a> {
    fn from(value: BluetoothGapFrame<'a>) -> Self {
        Self::Gap(value)
    }
}

impl<'a> From<BluetoothLeAirFrame<'a>> for BluetoothCanonicalFrame<'a> {
    fn from(value: BluetoothLeAirFrame<'a>) -> Self {
        Self::LeAir(value)
    }
}
