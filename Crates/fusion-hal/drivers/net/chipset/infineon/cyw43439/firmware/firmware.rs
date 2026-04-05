//! CYW43439 firmware and board-configuration assets.

const CYW43439_COMBINED_WIFI_ALIGNMENT_BYTES: usize = 512;

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

/// One CYW43439-specific packed WLAN firmware image.
///
/// Some vendor distributions ship the main WLAN firmware padded up to a 512-byte boundary with
/// the CLM blob appended immediately after that padded window. That layout is CYW43439-family
/// baggage, so the split helper lives here instead of being reinvented inside each PAL binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cyw43439PackedWlanFirmwareImage {
    pub image: &'static [u8],
    pub firmware_len: usize,
    pub clm_len: usize,
}

impl Cyw43439PackedWlanFirmwareImage {
    #[must_use]
    pub const fn padded_firmware_len(self) -> usize {
        let align = CYW43439_COMBINED_WIFI_ALIGNMENT_BYTES;
        ((self.firmware_len + align - 1) / align) * align
    }

    #[must_use]
    pub const fn is_layout_valid(self) -> bool {
        let padded = self.padded_firmware_len();
        match padded.checked_add(self.clm_len) {
            Some(required) => self.image.len() >= required,
            None => false,
        }
    }

    #[must_use]
    pub fn firmware_image(self) -> Option<&'static [u8]> {
        if !self.is_layout_valid() {
            return None;
        }
        Some(&self.image[..self.firmware_len])
    }

    #[must_use]
    pub fn clm_image(self) -> Option<&'static [u8]> {
        if !self.is_layout_valid() {
            return None;
        }
        let start = self.padded_firmware_len();
        let end = start + self.clm_len;
        Some(&self.image[start..end])
    }
}

#[cfg(test)]
mod tests {
    use super::Cyw43439PackedWlanFirmwareImage;

    #[test]
    fn packed_wlan_image_splits_firmware_and_clm() {
        static IMAGE: [u8; 514] = {
            let mut bytes = [0_u8; 514];
            bytes[0] = 1;
            bytes[1] = 2;
            bytes[2] = 3;
            bytes[3] = 4;
            bytes[512] = 9;
            bytes[513] = 10;
            bytes
        };
        let packed = Cyw43439PackedWlanFirmwareImage {
            image: &IMAGE,
            firmware_len: 4,
            clm_len: 2,
        };

        assert!(packed.is_layout_valid());
        assert_eq!(packed.firmware_image(), Some(&[1, 2, 3, 4][..]));
        assert_eq!(packed.clm_image(), Some(&[9, 10][..]));
    }

    #[test]
    fn packed_wlan_image_rejects_truncated_layout() {
        static IMAGE: [u8; 4] = [1, 2, 3, 4];
        let packed = Cyw43439PackedWlanFirmwareImage {
            image: &IMAGE,
            firmware_len: 4,
            clm_len: 2,
        };

        assert!(!packed.is_layout_valid());
        assert_eq!(packed.firmware_image(), None);
        assert_eq!(packed.clm_image(), None);
    }
}
