//! Canonical Bluetooth GATT frames.

pub use super::super::{
    BluetoothGattAttributeValue,
    BluetoothGattCharacteristicHandle,
    BluetoothGattCharacteristicRange,
    BluetoothGattDescriptorRange,
    BluetoothGattServiceDefinition,
    BluetoothGattServiceHandle,
    BluetoothGattServiceRange,
};

/// One canonical GATT frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BluetoothGattFrame<'a> {
    PrimaryServices(&'a [BluetoothGattServiceRange]),
    Characteristics(&'a [BluetoothGattCharacteristicRange]),
    Descriptors(&'a [BluetoothGattDescriptorRange]),
    AttributeValue(BluetoothGattAttributeValue<'a>),
    Notification {
        characteristic: BluetoothGattCharacteristicHandle,
        value: &'a [u8],
    },
    Indication {
        characteristic: BluetoothGattCharacteristicHandle,
        value: &'a [u8],
    },
    PublishServices(&'a [BluetoothGattServiceDefinition<'a>]),
    ServiceChanged {
        service: BluetoothGattServiceHandle,
        end_group_handle: u16,
    },
}
