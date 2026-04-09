//! Hardware-facing USB substrate contract consumed by the universal USB driver.

use fusion_hal::contract::drivers::bus::usb::{
    ThunderboltContract,
    ThunderboltMetadata,
    Usb4Contract,
    Usb4Metadata,
    Usb4RouterState,
    UsbCoreContract,
    UsbCoreMetadata,
    UsbDeviceControllerContract,
    UsbError,
    UsbPdContract,
    UsbPdContractState,
    UsbSupport,
    UsbTopologyContract,
    UsbTypecPortContract,
    UsbTypecPortStatus,
    UsbHostControllerContract,
};

/// Hardware-facing contract for one USB substrate implementation.
pub trait UsbHardware {
    /// Concrete host-controller surface surfaced by this substrate.
    type HostController: UsbHostControllerContract;
    /// Concrete device-controller surface surfaced by this substrate.
    type DeviceController: UsbDeviceControllerContract;

    /// Reports the truthful coarse USB surface for this substrate.
    fn support() -> UsbSupport;

    /// Returns the truthful shared core metadata for this substrate.
    fn core_metadata() -> UsbCoreMetadata;

    /// Returns the host-controller surface when this substrate supports host mode.
    ///
    /// # Errors
    ///
    /// Returns one honest error when host-controller access cannot be realized.
    fn host_controller() -> Result<Option<Self::HostController>, UsbError>;

    /// Returns the device-controller surface when this substrate supports device mode.
    ///
    /// # Errors
    ///
    /// Returns one honest error when device-controller access cannot be realized.
    fn device_controller() -> Result<Option<Self::DeviceController>, UsbError>;
}

/// Optional hardware-facing topology surface.
pub trait UsbHardwareTopology: UsbHardware {
    /// Returns the truthful topology port count.
    fn topology_port_count() -> usize;

    /// Returns one truthful topology status snapshot for the requested port.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the port is invalid or unavailable.
    fn topology_port_status(
        port: fusion_hal::contract::drivers::bus::usb::UsbPortId,
    ) -> Result<fusion_hal::contract::drivers::bus::usb::UsbPortStatus, UsbError>;
}

/// Optional hardware-facing Type-C surface.
pub trait UsbHardwareTypec: UsbHardware {
    /// Returns one truthful Type-C status snapshot.
    ///
    /// The current universal USB substrate is stateless, so hardware-facing Type-C snapshots must
    /// be self-contained and `'static` rather than borrowing from one controller instance. If we
    /// later move to stateful controller instances, this boundary can widen honestly.
    ///
    /// # Errors
    ///
    /// Returns one honest error when Type-C state is unavailable.
    fn typec_status() -> Result<UsbTypecPortStatus<'static>, UsbError>;
}

/// Optional hardware-facing USB PD surface.
pub trait UsbHardwarePd: UsbHardware {
    /// Returns one truthful PD contract state snapshot.
    ///
    /// The current universal USB substrate is stateless, so PD snapshots must be self-contained
    /// and `'static` rather than borrowing from one controller instance. If we later move to
    /// stateful controller instances, this boundary can widen honestly.
    ///
    /// # Errors
    ///
    /// Returns one honest error when PD state is unavailable.
    fn pd_contract_state() -> Result<UsbPdContractState<'static>, UsbError>;
}

/// Optional hardware-facing USB4 surface.
pub trait UsbHardwareUsb4: UsbHardware {
    /// Returns the current USB4 metadata snapshot.
    fn usb4_metadata() -> Usb4Metadata;

    /// Returns the current USB4 router/fabric state.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the state cannot be observed.
    fn usb4_state() -> Result<Usb4RouterState, UsbError>;
}

/// Optional hardware-facing Thunderbolt surface.
pub trait UsbHardwareThunderbolt: UsbHardware {
    /// Returns the current Thunderbolt metadata snapshot.
    fn thunderbolt_metadata() -> ThunderboltMetadata;

    /// Returns whether Thunderbolt mode is currently active.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the state cannot be characterized.
    fn thunderbolt_active() -> Result<bool, UsbError>;
}

#[allow(dead_code)]
fn _trait_shape_check<T>()
where
    T: UsbCoreContract
        + UsbTopologyContract
        + UsbTypecPortContract
        + UsbPdContract
        + Usb4Contract
        + ThunderboltContract,
{
}
