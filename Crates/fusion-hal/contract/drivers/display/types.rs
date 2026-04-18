//! Shared display capability, descriptor, and presentation vocabulary.

use super::core::{
    DisplayColorSpace,
    DisplayConnectorKind,
    DisplayFeature,
    DisplayFrameId,
    DisplayMode,
    DisplayPixelFormat,
    DisplayPowerState,
    DisplayQuantization,
    DisplayRegion,
    DisplaySurfaceId,
    DisplaySurfaceKind,
    DisplayTiming,
};

/// Stable display-output identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayOutputId(pub u16);

/// Static descriptor for one surfaced display output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayOutputDescriptor {
    pub id: DisplayOutputId,
    /// Stable driver-declared output label such as `hdmi-0`.
    ///
    /// This is connector/output identity, not sink-derived identity like EDID model strings.
    pub name: &'static str,
    pub connector: DisplayConnectorKind,
    pub hotplug_supported: bool,
}

/// Raw sink/control descriptor family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisplayRawDescriptorKind {
    Edid,
    Dpcd,
    DisplayId,
    Vendor,
    Other(u16),
}

/// One raw sink/control descriptor surfaced by one display path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayRawDescriptor<'a> {
    pub kind: DisplayRawDescriptorKind,
    pub bytes: &'a [u8],
}

/// Borrowed descriptor set currently known for one display sink/path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DisplayDescriptorSet<'a> {
    pub descriptors: &'a [DisplayRawDescriptor<'a>],
}

/// Supported pixel-format truth for one sink or port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DisplayPixelFormatSupport {
    pub rgb565: bool,
    pub rgb888: bool,
    pub bgr888: bool,
    pub xrgb8888: bool,
    pub argb8888: bool,
    pub xbgr8888: bool,
    pub abgr8888: bool,
    pub rgb101010: bool,
    pub bgr101010: bool,
}

/// Supported color-space truth for one sink or port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DisplayColorSpaceSupport {
    pub rgb: bool,
    pub ycbcr444: bool,
    pub ycbcr422: bool,
    pub ycbcr420: bool,
}

/// Supported quantization-range truth for one sink or port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DisplayQuantizationSupport {
    pub default: bool,
    pub full: bool,
    pub limited: bool,
}

/// Sink audio-capability truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DisplayAudioCapabilities {
    pub basic_pcm: bool,
    pub compressed_streams: bool,
    pub max_channels: u8,
    pub max_sample_rate_hz: Option<u32>,
}

/// Sink HDR-capability truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DisplayHdrCapabilities {
    pub hdr_static_metadata: bool,
    pub hdr10: bool,
    pub hdr10_plus: bool,
    pub dolby_vision: bool,
    pub hybrid_log_gamma: bool,
    pub max_luminance_nits: Option<u16>,
    pub min_luminance_nits: Option<u16>,
}

/// Sink variable-refresh capability truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DisplayVrrCapabilities {
    pub adaptive_sync: bool,
    pub min_refresh_hz: Option<u16>,
    pub max_refresh_hz: Option<u16>,
}

/// Sink scaling capability truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DisplayScalingCapabilities {
    pub source_scaling: bool,
    pub sink_scaling: bool,
    pub aspect_preserving: bool,
    pub integer_scaling: bool,
}

/// Sink content-protection capability truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DisplayProtectionCapabilities {
    pub hdcp_1_4: bool,
    pub hdcp_2_2: bool,
    pub hdcp_2_3: bool,
}

/// Capabilities currently known for one surfaced sink.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DisplaySinkCapabilities<'a> {
    pub modes: &'a [DisplayMode],
    pub preferred_mode: Option<DisplayMode>,
    pub max_pixel_clock_khz: Option<u32>,
    pub pixel_formats: DisplayPixelFormatSupport,
    pub color_spaces: DisplayColorSpaceSupport,
    pub quantization: DisplayQuantizationSupport,
    pub audio: DisplayAudioCapabilities,
    pub hdr: DisplayHdrCapabilities,
    pub vrr: DisplayVrrCapabilities,
    pub scaling: DisplayScalingCapabilities,
    pub content_protection: DisplayProtectionCapabilities,
}

/// Feature-management capability truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DisplayFeatureCapabilities {
    pub brightness: bool,
    pub contrast: bool,
    pub backlight: bool,
    pub mute: bool,
    pub input_select: bool,
}

/// Current display-control state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayControlState {
    pub connected: bool,
    pub descriptor_valid: bool,
    pub sink_power: DisplayPowerState,
}

/// One negotiation request from caller policy into sink/port truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayNegotiationRequest<'a> {
    pub preferred_modes: &'a [DisplayMode],
    pub preferred_pixel_formats: DisplayPixelFormatSupport,
    pub preferred_color_spaces: DisplayColorSpaceSupport,
    pub preferred_quantization: DisplayQuantizationSupport,
    pub require_audio: bool,
    pub prefer_hdr: bool,
    pub prefer_vrr: bool,
    pub allow_scaling: bool,
    pub allow_interlaced: bool,
}

/// Why one display negotiation result was selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisplayNegotiationReason {
    Requested,
    ClosestMatch,
    ScaledFallback,
    SafeFallback,
    ReducedCapabilities,
}

/// One active display configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayActiveConfig {
    pub mode: DisplayMode,
    pub timing: DisplayTiming,
    pub pixel_format: DisplayPixelFormat,
    pub color_space: DisplayColorSpace,
    pub quantization: DisplayQuantization,
    pub audio_enabled: bool,
    pub hdr_enabled: bool,
    pub vrr_enabled: bool,
}

/// Selected result of sink/port negotiation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayNegotiationResult {
    pub config: DisplayActiveConfig,
    pub reason: DisplayNegotiationReason,
}

/// Static descriptor for one surfaced display port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayPortDescriptor {
    pub connector: DisplayConnectorKind,
    pub hotplug_supported: bool,
    pub hotplug_event_supported: bool,
    pub cpu_upload_supported: bool,
    pub direct_scanout_supported: bool,
    pub page_flip_supported: bool,
    pub partial_update_supported: bool,
    pub vblank_wait_supported: bool,
}

/// Current display-port state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayPortState {
    pub connected: bool,
    pub enabled: bool,
    pub blanked: bool,
    pub configured: bool,
    pub active_config: Option<DisplayActiveConfig>,
}

/// Static capability truth for one display port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayPortCapabilities {
    pub max_width: u32,
    pub max_height: u32,
    pub max_refresh_hz: u32,
    pub supported_pixel_formats: DisplayPixelFormatSupport,
    pub min_stride_alignment: u32,
    pub min_surface_alignment: u32,
}

/// Why one requested display configuration was rejected or degraded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisplayConfigError {
    UnsupportedMode,
    UnsupportedTiming,
    UnsupportedPixelFormat,
    UnsupportedColorSpace,
    UnsupportedQuantization,
    BandwidthExceeded,
    SurfaceIncompatible,
    NotReady,
}

/// Kind of hotplug/state-change event surfaced by one display port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisplayHotplugEventKind {
    Connected,
    Disconnected,
    Changed,
}

/// One surfaced hotplug/state-change event from a display port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayHotplugEvent {
    pub generation: u64,
    pub kind: DisplayHotplugEventKind,
}

/// Backing kind attached to one display surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisplaySurfaceBacking {
    CpuVirtual { address: usize, len_bytes: usize },
    CpuPhysical { address: u64, len_bytes: u64 },
    ExternalHandle(u64),
}

/// Surface binding accepted by one display port for scanout/present work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplaySurfaceBinding {
    pub id: DisplaySurfaceId,
    pub surface_kind: DisplaySurfaceKind,
    pub width: u32,
    pub height: u32,
    pub stride_bytes: u32,
    pub pixel_format: DisplayPixelFormat,
    pub backing: DisplaySurfaceBacking,
}

/// Borrowed frame view for upload-oriented display paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayFrameView<'a> {
    pub width: u32,
    pub height: u32,
    pub stride_bytes: u32,
    pub pixel_format: DisplayPixelFormat,
    pub bytes: &'a [u8],
}

/// Present request against one display port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayPresentRequest {
    pub surface: Option<DisplaySurfaceId>,
    pub wait_for_vblank: bool,
    pub allow_tearing: bool,
    pub region: Option<DisplayRegion>,
}

/// Result of one upload-oriented display operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayUploadReport {
    pub bytes_uploaded: u32,
    pub region_applied: Option<DisplayRegion>,
}

/// Result of one present-oriented display operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayPresentReport {
    pub presented: bool,
    pub frame_id: DisplayFrameId,
    pub vblank_sequence: Option<u64>,
}

/// Global output transform within one machine layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisplayOutputTransform {
    Identity,
    Rotate90,
    Rotate180,
    Rotate270,
    FlipHorizontal,
    FlipVertical,
}

/// Requested placement for one output within machine display space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayOutputPlacement {
    pub output: DisplayOutputId,
    pub origin_x: i32,
    pub origin_y: i32,
    pub logical_width: u32,
    pub logical_height: u32,
    pub scale_milli: u32,
    pub transform: DisplayOutputTransform,
    pub enabled: bool,
}

/// Current machine display-composition state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DisplayLayoutState {
    pub generation: u64,
    pub output_count: u16,
    pub surface_count: u16,
    pub primary_output: Option<DisplayOutputId>,
}

/// Requested machine layout update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DisplayLayoutConfig<'a> {
    pub outputs: &'a [DisplayOutputPlacement],
    pub primary_output: Option<DisplayOutputId>,
}

/// Why one machine layout was rejected during preflight validation.
///
/// This stays separate from `DisplayError` because layout validation is not an operational driver
/// failure. It is typed contract rejection for one proposed machine composition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisplayLayoutValidationError {
    UnknownOutput,
    DuplicateOutput,
    OverlappingOutputs,
    InvalidPrimaryOutput,
    UnsupportedTransform,
    UnsupportedScale,
    NotReady,
}

/// Placement for one surfaced composition surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplaySurfacePlacement {
    pub output: DisplayOutputId,
    pub region: Option<DisplayRegion>,
    pub z_index: i32,
    pub visible: bool,
}

/// One machine-wide layout presentation request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DisplayLayoutPresentRequest<'a> {
    pub outputs: &'a [DisplayOutputId],
    pub wait_for_vblank: bool,
    pub allow_tearing: bool,
}

/// Result of one machine-wide layout presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DisplayLayoutPresentReport {
    pub presented_outputs: u16,
    pub generation: u64,
}

impl DisplayFeatureCapabilities {
    #[must_use]
    pub const fn supports(self, feature: DisplayFeature) -> bool {
        match feature {
            DisplayFeature::Brightness => self.brightness,
            DisplayFeature::Contrast => self.contrast,
            DisplayFeature::Backlight => self.backlight,
            DisplayFeature::Mute => self.mute,
            DisplayFeature::InputSelect => self.input_select,
            DisplayFeature::Other(_) => false,
        }
    }
}

impl DisplayPixelFormatSupport {
    #[must_use]
    pub const fn supports(self, format: DisplayPixelFormat) -> bool {
        match format {
            DisplayPixelFormat::Rgb565 => self.rgb565,
            DisplayPixelFormat::Rgb888 => self.rgb888,
            DisplayPixelFormat::Bgr888 => self.bgr888,
            DisplayPixelFormat::Xrgb8888 => self.xrgb8888,
            DisplayPixelFormat::Argb8888 => self.argb8888,
            DisplayPixelFormat::Xbgr8888 => self.xbgr8888,
            DisplayPixelFormat::Abgr8888 => self.abgr8888,
            DisplayPixelFormat::Rgb101010 => self.rgb101010,
            DisplayPixelFormat::Bgr101010 => self.bgr101010,
            DisplayPixelFormat::Other(_) => false,
        }
    }
}

impl DisplayColorSpaceSupport {
    #[must_use]
    pub const fn supports(self, color_space: DisplayColorSpace) -> bool {
        match color_space {
            DisplayColorSpace::Rgb => self.rgb,
            DisplayColorSpace::Ycbcr444 => self.ycbcr444,
            DisplayColorSpace::Ycbcr422 => self.ycbcr422,
            DisplayColorSpace::Ycbcr420 => self.ycbcr420,
            DisplayColorSpace::Other(_) => false,
        }
    }
}

impl DisplayQuantizationSupport {
    #[must_use]
    pub const fn supports(self, quantization: DisplayQuantization) -> bool {
        match quantization {
            DisplayQuantization::Default => self.default,
            DisplayQuantization::Full => self.full,
            DisplayQuantization::Limited => self.limited,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_caps_report_supported_feature_truth() {
        let caps = DisplayFeatureCapabilities {
            brightness: true,
            contrast: false,
            backlight: false,
            mute: true,
            input_select: false,
        };
        assert!(caps.supports(DisplayFeature::Brightness));
        assert!(!caps.supports(DisplayFeature::Contrast));
        assert!(caps.supports(DisplayFeature::Mute));
    }
}
