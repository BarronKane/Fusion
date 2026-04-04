//! Hardware-facing CYW43439 controller-plumbing contracts.

use bitflags::bitflags;

use crate::contract::drivers::net::bluetooth::{
    BluetoothAdapterDescriptor,
    BluetoothAdapterId,
    BluetoothError,
    BluetoothSupport,
};

bitflags! {
    /// Truthful board/controller plumbing surfaced for one CYW43439 binding.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Cyw43439ControllerCaps: u32 {
        const CLAIM_CONTROLLER          = 1 << 0;
        const POWER_CONTROL             = 1 << 1;
        const RESET_CONTROL             = 1 << 2;
        const WAKE_CONTROL              = 1 << 3;
        const IRQ_WAIT                  = 1 << 4;
        const IRQ_ACKNOWLEDGE           = 1 << 5;
        const TRANSPORT_WRITE           = 1 << 6;
        const TRANSPORT_READ            = 1 << 7;
        const FIRMWARE_IMAGE            = 1 << 8;
        const NVRAM_IMAGE               = 1 << 9;
        const TIMING_DELAY              = 1 << 10;
    }
}

/// Hardware-facing contract for one CYW43439 controller binding.
///
/// This is intentionally lower than the public Bluetooth contract. The CYW43439 driver owns the
/// controller state machine and Bluetooth semantics; PAL-backed implementations only surface the
/// truthful board wiring, controller control, and transport hooks needed to run the chip.
pub trait Cyw43439Hardware {
    /// Reports the truthful Bluetooth surface for this substrate.
    fn support(&self) -> BluetoothSupport;

    /// Returns the statically or dynamically surfaced Bluetooth adapter descriptors.
    fn adapters(&self) -> &'static [BluetoothAdapterDescriptor];

    /// Returns the truthful controller-plumbing capability surface for one adapter binding.
    fn controller_caps(&self, adapter: BluetoothAdapterId) -> Cyw43439ControllerCaps;

    /// Claims one surfaced controller binding exclusively.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the adapter is invalid, unsupported, or already claimed.
    fn claim_controller(&mut self, adapter: BluetoothAdapterId) -> Result<(), BluetoothError>;

    /// Releases one previously claimed controller binding.
    fn release_controller(&mut self, adapter: BluetoothAdapterId);

    /// Returns whether the controller rail is currently powered.
    ///
    /// # Errors
    ///
    /// Returns one honest error when power state cannot be queried.
    fn controller_powered(&self, adapter: BluetoothAdapterId) -> Result<bool, BluetoothError>;

    /// Powers the controller rail on or off.
    ///
    /// # Errors
    ///
    /// Returns one honest error when power control is unsupported or fails.
    fn set_controller_powered(
        &mut self,
        adapter: BluetoothAdapterId,
        powered: bool,
    ) -> Result<(), BluetoothError>;

    /// Asserts or deasserts the controller reset line.
    ///
    /// # Errors
    ///
    /// Returns one honest error when reset control is unsupported or fails.
    fn set_controller_reset(
        &mut self,
        adapter: BluetoothAdapterId,
        asserted: bool,
    ) -> Result<(), BluetoothError>;

    /// Asserts or deasserts the controller wake line.
    ///
    /// # Errors
    ///
    /// Returns one honest error when wake control is unsupported or fails.
    fn set_controller_wake(
        &mut self,
        adapter: BluetoothAdapterId,
        awake: bool,
    ) -> Result<(), BluetoothError>;

    /// Waits for one controller interrupt indication.
    ///
    /// # Errors
    ///
    /// Returns one honest error when IRQ waiting is unsupported or fails.
    fn wait_for_controller_irq(
        &mut self,
        adapter: BluetoothAdapterId,
        timeout_ms: Option<u32>,
    ) -> Result<bool, BluetoothError>;

    /// Acknowledges one pending controller interrupt indication.
    ///
    /// # Errors
    ///
    /// Returns one honest error when IRQ acknowledge is unsupported or fails.
    fn acknowledge_controller_irq(
        &mut self,
        adapter: BluetoothAdapterId,
    ) -> Result<(), BluetoothError>;

    /// Writes one raw controller transport frame.
    ///
    /// # Errors
    ///
    /// Returns one honest error when transport writes are unsupported or fail.
    fn write_controller_transport(
        &mut self,
        adapter: BluetoothAdapterId,
        payload: &[u8],
    ) -> Result<(), BluetoothError>;

    /// Reads one raw controller transport frame into caller-owned storage.
    ///
    /// # Errors
    ///
    /// Returns one honest error when transport reads are unsupported or fail.
    fn read_controller_transport(
        &mut self,
        adapter: BluetoothAdapterId,
        out: &mut [u8],
    ) -> Result<usize, BluetoothError>;

    /// Returns one optional controller firmware image.
    ///
    /// # Errors
    ///
    /// Returns one honest error when firmware provisioning itself fails.
    fn firmware_image(
        &self,
        adapter: BluetoothAdapterId,
    ) -> Result<Option<&'static [u8]>, BluetoothError>;

    /// Returns one optional controller NVRAM/config image.
    ///
    /// # Errors
    ///
    /// Returns one honest error when NVRAM provisioning itself fails.
    fn nvram_image(
        &self,
        adapter: BluetoothAdapterId,
    ) -> Result<Option<&'static [u8]>, BluetoothError>;

    /// Sleeps for one board-truthful delay interval.
    fn delay_ms(&self, milliseconds: u32);
}
