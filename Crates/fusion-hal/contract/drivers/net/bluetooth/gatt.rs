//! GATT client and server control contracts.

use super::BluetoothConnectionId;
use super::BluetoothError;
use super::BluetoothGattAttributeValue;
use super::BluetoothGattCharacteristicHandle;
use super::BluetoothGattCharacteristicRange;
use super::BluetoothGattDescriptorRange;
use super::BluetoothGattServiceDefinition;
use super::BluetoothGattServiceHandle;
use super::BluetoothGattServiceRange;
use super::BluetoothOwnedAdapterContract;

/// GATT client control for one opened Bluetooth adapter.
pub trait BluetoothGattClientContract: BluetoothOwnedAdapterContract {
    /// Discovers primary services into caller-owned storage.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when discovery fails.
    fn discover_primary_services(
        &mut self,
        connection: BluetoothConnectionId,
        out: &mut [BluetoothGattServiceRange],
    ) -> Result<usize, BluetoothError>;

    /// Discovers characteristics under one service into caller-owned storage.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when discovery fails.
    fn discover_characteristics(
        &mut self,
        connection: BluetoothConnectionId,
        service: BluetoothGattServiceHandle,
        out: &mut [BluetoothGattCharacteristicRange],
    ) -> Result<usize, BluetoothError>;

    /// Discovers descriptors under one characteristic into caller-owned storage.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when discovery fails.
    fn discover_descriptors(
        &mut self,
        connection: BluetoothConnectionId,
        characteristic: BluetoothGattCharacteristicHandle,
        out: &mut [BluetoothGattDescriptorRange],
    ) -> Result<usize, BluetoothError>;

    /// Reads one characteristic value into caller-owned storage.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the read fails.
    fn read_characteristic<'a>(
        &mut self,
        connection: BluetoothConnectionId,
        characteristic: BluetoothGattCharacteristicHandle,
        out: &'a mut [u8],
    ) -> Result<BluetoothGattAttributeValue<'a>, BluetoothError>;

    /// Writes one characteristic value.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when the write fails.
    fn write_characteristic(
        &mut self,
        connection: BluetoothConnectionId,
        characteristic: BluetoothGattCharacteristicHandle,
        value: &[u8],
        with_response: bool,
    ) -> Result<(), BluetoothError>;

    /// Subscribes to characteristic notifications or indications.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when subscription fails.
    fn subscribe(
        &mut self,
        connection: BluetoothConnectionId,
        characteristic: BluetoothGattCharacteristicHandle,
        notify: bool,
        indicate: bool,
    ) -> Result<(), BluetoothError>;
}

/// GATT server control for one opened Bluetooth adapter.
pub trait BluetoothGattServerContract: BluetoothOwnedAdapterContract {
    /// Publishes one local GATT database definition.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when publication fails.
    fn publish_services(
        &mut self,
        services: &[BluetoothGattServiceDefinition<'_>],
    ) -> Result<(), BluetoothError>;

    /// Sends one characteristic notification.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when notification fails.
    fn notify(
        &mut self,
        connection: BluetoothConnectionId,
        characteristic: BluetoothGattCharacteristicHandle,
        value: &[u8],
    ) -> Result<(), BluetoothError>;

    /// Sends one characteristic indication.
    ///
    /// # Errors
    ///
    /// Returns one honest backend error when indication fails.
    fn indicate(
        &mut self,
        connection: BluetoothConnectionId,
        characteristic: BluetoothGattCharacteristicHandle,
        value: &[u8],
    ) -> Result<(), BluetoothError>;
}
