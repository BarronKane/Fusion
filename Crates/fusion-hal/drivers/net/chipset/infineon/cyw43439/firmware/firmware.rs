//! CYW43439 firmware and board-configuration assets.

/// Optional Bluetooth-side firmware artifacts for CYW43439.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cyw43439BluetoothFirmwareAssets {
    /// Optional patch image applied over the Bluetooth transport.
    pub patch_image: Option<&'static [u8]>,
}

/// Optional WLAN-side firmware artifacts for CYW43439.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cyw43439WlanFirmwareAssets {
    /// Mandatory WLAN firmware image once the transport path is real.
    pub firmware_image: Option<&'static [u8]>,
    /// Board-specific NVRAM/configuration payload.
    pub nvram_image: Option<&'static [u8]>,
    /// Optional CLM/regulatory payload if the selected firmware path requires it.
    pub clm_image: Option<&'static [u8]>,
}

/// Full firmware/configuration asset bundle for one CYW43439 combo chip.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cyw43439FirmwareAssets {
    pub bluetooth: Cyw43439BluetoothFirmwareAssets,
    pub wifi: Cyw43439WlanFirmwareAssets,
}
