//! Shared display driver-family vocabulary.

/// Stable surfaced display text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayText {
    bytes: [u8; 32],
    len: u8,
}

impl DisplayText {
    #[must_use]
    pub const fn new(bytes: [u8; 32], len: u8) -> Self {
        Self { bytes, len }
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }

    #[must_use]
    pub const fn len(self) -> u8 {
        self.len
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.len == 0
    }
}

/// Connector family surfaced by one public display port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisplayConnectorKind {
    Vga,
    Dvi,
    Hdmi,
    DisplayPort,
    EmbeddedDisplayPort,
    Other(u16),
}

/// Stable identity for one surfaced display sink.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayIdentity {
    pub manufacturer_id: Option<u16>,
    pub product_code: Option<u16>,
    pub serial_number: Option<u32>,
    pub model_name: Option<DisplayText>,
    pub connector: DisplayConnectorKind,
}

/// Coarse sink power state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisplayPowerState {
    On,
    Standby,
    Suspend,
    Off,
}

/// Pixel format accepted or produced by one display path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisplayPixelFormat {
    Rgb565,
    Rgb888,
    Bgr888,
    Xrgb8888,
    Argb8888,
    Xbgr8888,
    Abgr8888,
    Rgb101010,
    Bgr101010,
    Other(u32),
}

/// Sink color encoding family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisplayColorSpace {
    Rgb,
    Ycbcr444,
    Ycbcr422,
    Ycbcr420,
    Other(u16),
}

/// Signal-range quantization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisplayQuantization {
    Default,
    Full,
    Limited,
}

/// One surfaced display mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayMode {
    pub width: u32,
    pub height: u32,
    pub refresh_hz_milli: u32,
    pub interlaced: bool,
    pub preferred: bool,
}

/// Sync polarity pair for one timing descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplaySyncPolarity {
    pub hsync_positive: bool,
    pub vsync_positive: bool,
}

/// One explicit display timing descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayTiming {
    pub pixel_clock_khz: u32,
    pub h_active: u32,
    pub h_front_porch: u32,
    pub h_sync_width: u32,
    pub h_back_porch: u32,
    pub v_active: u32,
    pub v_front_porch: u32,
    pub v_sync_width: u32,
    pub v_back_porch: u32,
    pub interlaced: bool,
    pub polarity: DisplaySyncPolarity,
}

impl DisplayTiming {
    #[must_use]
    pub const fn horizontal_total(self) -> u32 {
        self.h_active + self.h_front_porch + self.h_sync_width + self.h_back_porch
    }

    #[must_use]
    pub const fn vertical_total(self) -> u32 {
        self.v_active + self.v_front_porch + self.v_sync_width + self.v_back_porch
    }
}

/// One rectangular display region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Shared monitor-management feature families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisplayFeature {
    Brightness,
    Contrast,
    Backlight,
    Mute,
    InputSelect,
    Other(u16),
}

/// Value surfaced or accepted by one display-management feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisplayFeatureValue {
    Bool(bool),
    Percent(u8),
    Enum(u32),
}

/// Surface-binding family accepted by one display port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisplaySurfaceKind {
    CpuLinear,
    DirectScanout,
    PageFlippable,
    ExternalHandle,
}

/// Stable surface identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplaySurfaceId(pub u64);

/// Opaque frame/present identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayFrameId(pub u64);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_text_exposes_visible_slice() {
        let text = DisplayText::new(
            [
                b'F', b'u', b's', b'i', b'o', b'n', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ],
            6,
        );
        assert_eq!(text.as_bytes(), b"Fusion");
    }

    #[test]
    fn timing_totals_add_visible_and_blanking_intervals() {
        let timing = DisplayTiming {
            pixel_clock_khz: 25_175,
            h_active: 640,
            h_front_porch: 16,
            h_sync_width: 96,
            h_back_porch: 48,
            v_active: 480,
            v_front_porch: 10,
            v_sync_width: 2,
            v_back_porch: 33,
            interlaced: false,
            polarity: DisplaySyncPolarity {
                hsync_positive: false,
                vsync_positive: false,
            },
        };
        assert_eq!(timing.horizontal_total(), 800);
        assert_eq!(timing.vertical_total(), 525);
    }
}
