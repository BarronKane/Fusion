//! Shared EDID/CTA parsing helpers for connector display drivers.

use crate::contract::drivers::display::{
    DisplayAudioCapabilities,
    DisplayColorSpace,
    DisplayColorSpaceSupport,
    DisplayConnectorKind,
    DisplayDescriptorSet,
    DisplayHdrCapabilities,
    DisplayIdentity,
    DisplayMode,
    DisplayPixelFormat,
    DisplayPixelFormatSupport,
    DisplayPortCapabilities,
    DisplayProtectionCapabilities,
    DisplayQuantization,
    DisplayQuantizationSupport,
    DisplayRawDescriptorKind,
    DisplayScalingCapabilities,
    DisplaySinkCapabilities,
    DisplaySyncPolarity,
    DisplayText,
    DisplayTiming,
    DisplayVrrCapabilities,
};

pub(crate) const MAX_EDID_MODES: usize = 32;
pub(crate) const EDID_BLOCK_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct EdidModeRecord {
    mode: DisplayMode,
    timing: DisplayTiming,
}

impl EdidModeRecord {
    const EMPTY: Self = Self {
        mode: DisplayMode {
            width: 0,
            height: 0,
            refresh_hz_milli: 0,
            interlaced: false,
            preferred: false,
        },
        timing: DisplayTiming {
            pixel_clock_khz: 0,
            h_active: 0,
            h_front_porch: 0,
            h_sync_width: 0,
            h_back_porch: 0,
            v_active: 0,
            v_front_porch: 0,
            v_sync_width: 0,
            v_back_porch: 0,
            interlaced: false,
            polarity: DisplaySyncPolarity {
                hsync_positive: false,
                vsync_positive: false,
            },
        },
    };
}

#[derive(Debug, Clone)]
pub(crate) struct ParsedEdidSink {
    pub(crate) descriptors: DisplayDescriptorSet<'static>,
    pub(crate) descriptor_valid: bool,
    pub(crate) identity: DisplayIdentity,
    pub(crate) modes: [DisplayMode; MAX_EDID_MODES],
    pub(crate) timings: [DisplayTiming; MAX_EDID_MODES],
    pub(crate) mode_count: usize,
    pub(crate) max_pixel_clock_khz: Option<u32>,
    pub(crate) pixel_formats: DisplayPixelFormatSupport,
    pub(crate) color_spaces: DisplayColorSpaceSupport,
    pub(crate) quantization: DisplayQuantizationSupport,
    pub(crate) audio: DisplayAudioCapabilities,
    pub(crate) hdr: DisplayHdrCapabilities,
    pub(crate) vrr: DisplayVrrCapabilities,
    pub(crate) scaling: DisplayScalingCapabilities,
    pub(crate) protection: DisplayProtectionCapabilities,
}

impl ParsedEdidSink {
    fn empty(connector: DisplayConnectorKind, descriptors: DisplayDescriptorSet<'static>) -> Self {
        Self {
            descriptors,
            descriptor_valid: false,
            identity: DisplayIdentity {
                manufacturer_id: None,
                product_code: None,
                serial_number: None,
                model_name: None,
                connector,
            },
            modes: [EdidModeRecord::EMPTY.mode; MAX_EDID_MODES],
            timings: [EdidModeRecord::EMPTY.timing; MAX_EDID_MODES],
            mode_count: 0,
            max_pixel_clock_khz: None,
            pixel_formats: DisplayPixelFormatSupport {
                rgb565: true,
                rgb888: true,
                bgr888: true,
                xrgb8888: true,
                argb8888: true,
                xbgr8888: true,
                abgr8888: true,
                rgb101010: false,
                bgr101010: false,
            },
            color_spaces: DisplayColorSpaceSupport {
                rgb: true,
                ycbcr444: false,
                ycbcr422: false,
                ycbcr420: false,
            },
            quantization: DisplayQuantizationSupport {
                default: true,
                full: false,
                limited: false,
            },
            audio: DisplayAudioCapabilities::default(),
            hdr: DisplayHdrCapabilities::default(),
            vrr: DisplayVrrCapabilities::default(),
            scaling: DisplayScalingCapabilities::default(),
            protection: DisplayProtectionCapabilities::default(),
        }
    }

    pub(crate) fn timing_for_mode(&self, mode: DisplayMode) -> Option<DisplayTiming> {
        for index in 0..self.mode_count {
            if same_mode(self.modes[index], mode) {
                return Some(self.timings[index]);
            }
        }
        None
    }

    pub(crate) fn supports_mode(&self, mode: DisplayMode) -> bool {
        self.timing_for_mode(mode).is_some()
    }

    fn push_mode(&mut self, mut mode: DisplayMode, timing: DisplayTiming, preferred: bool) {
        for index in 0..self.mode_count {
            if same_mode(self.modes[index], mode) {
                if preferred {
                    self.modes[index].preferred = true;
                }
                return;
            }
        }
        if self.mode_count == MAX_EDID_MODES {
            return;
        }
        mode.preferred = preferred;
        self.max_pixel_clock_khz = Some(
            self.max_pixel_clock_khz
                .map_or(timing.pixel_clock_khz, |current| {
                    current.max(timing.pixel_clock_khz)
                }),
        );
        self.modes[self.mode_count] = mode;
        self.timings[self.mode_count] = timing;
        self.mode_count += 1;
    }

    pub(crate) fn sink_capabilities(&self) -> DisplaySinkCapabilities<'_> {
        DisplaySinkCapabilities {
            modes: &self.modes[..self.mode_count],
            preferred_mode: self.modes[..self.mode_count]
                .iter()
                .copied()
                .find(|mode| mode.preferred),
            max_pixel_clock_khz: self.max_pixel_clock_khz,
            pixel_formats: self.pixel_formats,
            color_spaces: self.color_spaces,
            quantization: self.quantization,
            audio: self.audio,
            hdr: self.hdr,
            vrr: self.vrr,
            scaling: self.scaling,
            content_protection: self.protection,
        }
    }

    pub(crate) fn constrain_for_rgb_only(&mut self) {
        self.color_spaces = DisplayColorSpaceSupport {
            rgb: true,
            ycbcr444: false,
            ycbcr422: false,
            ycbcr420: false,
        };
        self.audio = DisplayAudioCapabilities::default();
        self.hdr = DisplayHdrCapabilities::default();
        self.vrr = DisplayVrrCapabilities::default();
        self.scaling = DisplayScalingCapabilities::default();
        self.protection = DisplayProtectionCapabilities::default();
    }
}

pub(crate) fn parse_edid_sink(
    connector: DisplayConnectorKind,
    descriptors: DisplayDescriptorSet<'static>,
) -> ParsedEdidSink {
    let mut sink = ParsedEdidSink::empty(connector, descriptors);
    let Some(edid_bytes) = find_edid_bytes(descriptors) else {
        return sink;
    };

    if edid_bytes.len() < EDID_BLOCK_BYTES || edid_bytes.len() % EDID_BLOCK_BYTES != 0 {
        return sink;
    }
    if !has_edid_header(edid_bytes) {
        return sink;
    }
    if !edid_bytes
        .chunks_exact(EDID_BLOCK_BYTES)
        .all(edid_checksum_valid)
    {
        return sink;
    }

    let declared_extensions = usize::from(edid_bytes[126]);
    let available_extensions = edid_bytes.len() / EDID_BLOCK_BYTES - 1;
    if available_extensions < declared_extensions {
        return sink;
    }

    sink.descriptor_valid = true;
    sink.identity.manufacturer_id = Some(parse_manufacturer_id(edid_bytes[8], edid_bytes[9]));
    sink.identity.product_code = Some(u16::from_le_bytes([edid_bytes[10], edid_bytes[11]]));
    sink.identity.serial_number = Some(u32::from_le_bytes([
        edid_bytes[12],
        edid_bytes[13],
        edid_bytes[14],
        edid_bytes[15],
    ]));
    sink.identity.model_name = parse_model_name(edid_bytes);

    for descriptor in edid_bytes[54..126].chunks_exact(18) {
        if let Some((mode, timing)) = parse_detailed_timing(descriptor) {
            let preferred = sink.mode_count == 0;
            sink.push_mode(mode, timing, preferred);
        }
    }

    for extension in edid_bytes[EDID_BLOCK_BYTES..].chunks_exact(EDID_BLOCK_BYTES) {
        if extension.first().copied() == Some(0x02) {
            parse_cta_extension(extension, &mut sink);
        }
    }

    sink
}

fn parse_cta_extension(extension: &[u8], sink: &mut ParsedEdidSink) {
    if extension.len() != EDID_BLOCK_BYTES {
        return;
    }

    let dtd_start = usize::from(extension[2]).max(4).min(EDID_BLOCK_BYTES);
    let flags = extension[3];
    sink.audio.basic_pcm |= flags & 0x40 != 0;
    sink.color_spaces.ycbcr444 |= flags & 0x20 != 0;
    sink.color_spaces.ycbcr422 |= flags & 0x10 != 0;

    let mut index = 4;
    while index < dtd_start {
        let header = extension[index];
        if header == 0 {
            break;
        }
        let length = usize::from(header & 0x1f);
        let tag = header >> 5;
        let start = index + 1;
        let end = start.saturating_add(length).min(dtd_start);
        let payload = &extension[start..end];
        match tag {
            1 => {
                if !payload.is_empty() {
                    sink.audio.basic_pcm = true;
                    for short in payload.chunks_exact(3) {
                        let channel_count = (short[0] & 0x07) + 1;
                        sink.audio.max_channels = sink.audio.max_channels.max(channel_count);
                    }
                }
            }
            2 => {
                for vic in payload {
                    let preferred = vic & 0x80 != 0 && !has_preferred_mode(sink);
                    if let Some((mode, timing)) = lookup_cea_mode(vic & 0x7f) {
                        sink.push_mode(mode, timing, preferred);
                    }
                }
            }
            7 => parse_cta_extended_block(payload, sink),
            _ => {}
        }
        index = end;
    }

    for descriptor in extension[dtd_start..].chunks_exact(18) {
        if let Some((mode, timing)) = parse_detailed_timing(descriptor) {
            let preferred = !has_preferred_mode(sink);
            sink.push_mode(mode, timing, preferred);
        }
    }
}

fn parse_cta_extended_block(payload: &[u8], sink: &mut ParsedEdidSink) {
    let Some((&extended_tag, rest)) = payload.split_first() else {
        return;
    };

    if extended_tag == 0x06 {
        sink.hdr.hdr_static_metadata = true;
        if let Some(&eotf) = rest.first() {
            sink.hdr.hdr10 |= eotf & 0x04 != 0;
            sink.hdr.hybrid_log_gamma |= eotf & 0x08 != 0;
        }
    }
}

fn find_edid_bytes(descriptors: DisplayDescriptorSet<'static>) -> Option<&'static [u8]> {
    descriptors
        .descriptors
        .iter()
        .find(|descriptor| descriptor.kind == DisplayRawDescriptorKind::Edid)
        .map(|descriptor| descriptor.bytes)
}

fn has_edid_header(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && bytes[..8] == [0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00]
}

fn edid_checksum_valid(block: &[u8]) -> bool {
    block.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte)) == 0
}

fn parse_manufacturer_id(msb: u8, lsb: u8) -> u16 {
    u16::from_be_bytes([msb, lsb])
}

fn parse_model_name(base_block: &[u8]) -> Option<DisplayText> {
    for descriptor in base_block[54..126].chunks_exact(18) {
        if descriptor[..5] == [0x00, 0x00, 0x00, 0xfc, 0x00] {
            return parse_display_text(&descriptor[5..18]);
        }
    }
    None
}

fn parse_display_text(bytes: &[u8]) -> Option<DisplayText> {
    let mut raw = [0u8; 32];
    let mut len = 0usize;
    for byte in bytes {
        if *byte == 0x0a || *byte == 0x00 {
            break;
        }
        if len == raw.len() {
            break;
        }
        raw[len] = *byte;
        len += 1;
    }
    while len > 0 && raw[len - 1] == b' ' {
        len -= 1;
    }
    if len == 0 {
        None
    } else {
        Some(DisplayText::new(raw, len as u8))
    }
}

fn parse_detailed_timing(descriptor: &[u8]) -> Option<(DisplayMode, DisplayTiming)> {
    if descriptor.len() != 18 {
        return None;
    }
    let pixel_clock_raw = u16::from_le_bytes([descriptor[0], descriptor[1]]);
    if pixel_clock_raw == 0 {
        return None;
    }

    let h_active = u32::from(descriptor[2]) | (u32::from(descriptor[4] & 0xf0) << 4);
    let h_blanking = u32::from(descriptor[3]) | (u32::from(descriptor[4] & 0x0f) << 8);
    let v_active = u32::from(descriptor[5]) | (u32::from(descriptor[7] & 0xf0) << 4);
    let v_blanking = u32::from(descriptor[6]) | (u32::from(descriptor[7] & 0x0f) << 8);
    let h_front_porch = u32::from(descriptor[8]) | (u32::from((descriptor[11] >> 6) & 0x03) << 8);
    let h_sync_width = u32::from(descriptor[9]) | (u32::from((descriptor[11] >> 4) & 0x03) << 8);
    let v_front_porch =
        u32::from(descriptor[10] >> 4) | (u32::from((descriptor[11] >> 2) & 0x03) << 4);
    let v_sync_width = u32::from(descriptor[10] & 0x0f) | (u32::from(descriptor[11] & 0x03) << 4);
    let h_back_porch = h_blanking.saturating_sub(h_front_porch + h_sync_width);
    let v_back_porch = v_blanking.saturating_sub(v_front_porch + v_sync_width);
    let flags = descriptor[17];
    let timing = DisplayTiming {
        pixel_clock_khz: u32::from(pixel_clock_raw) * 10,
        h_active,
        h_front_porch,
        h_sync_width,
        h_back_porch,
        v_active,
        v_front_porch,
        v_sync_width,
        v_back_porch,
        interlaced: flags & 0x80 != 0,
        polarity: DisplaySyncPolarity {
            hsync_positive: flags & 0x02 != 0,
            vsync_positive: flags & 0x04 != 0,
        },
    };
    Some((mode_from_timing(timing, false), timing))
}

pub(crate) fn lookup_cea_mode(vic: u8) -> Option<(DisplayMode, DisplayTiming)> {
    let timing = match vic {
        1 => DisplayTiming {
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
        },
        4 => DisplayTiming {
            pixel_clock_khz: 74_250,
            h_active: 1280,
            h_front_porch: 110,
            h_sync_width: 40,
            h_back_porch: 220,
            v_active: 720,
            v_front_porch: 5,
            v_sync_width: 5,
            v_back_porch: 20,
            interlaced: false,
            polarity: DisplaySyncPolarity {
                hsync_positive: true,
                vsync_positive: true,
            },
        },
        16 => DisplayTiming {
            pixel_clock_khz: 148_500,
            h_active: 1920,
            h_front_porch: 88,
            h_sync_width: 44,
            h_back_porch: 148,
            v_active: 1080,
            v_front_porch: 4,
            v_sync_width: 5,
            v_back_porch: 36,
            interlaced: false,
            polarity: DisplaySyncPolarity {
                hsync_positive: true,
                vsync_positive: true,
            },
        },
        19 => DisplayTiming {
            pixel_clock_khz: 74_250,
            h_active: 1280,
            h_front_porch: 440,
            h_sync_width: 40,
            h_back_porch: 220,
            v_active: 720,
            v_front_porch: 5,
            v_sync_width: 5,
            v_back_porch: 20,
            interlaced: false,
            polarity: DisplaySyncPolarity {
                hsync_positive: true,
                vsync_positive: true,
            },
        },
        31 => DisplayTiming {
            pixel_clock_khz: 148_500,
            h_active: 1920,
            h_front_porch: 528,
            h_sync_width: 44,
            h_back_porch: 148,
            v_active: 1080,
            v_front_porch: 4,
            v_sync_width: 5,
            v_back_porch: 36,
            interlaced: false,
            polarity: DisplaySyncPolarity {
                hsync_positive: true,
                vsync_positive: true,
            },
        },
        34 => DisplayTiming {
            pixel_clock_khz: 74_250,
            h_active: 1920,
            h_front_porch: 88,
            h_sync_width: 44,
            h_back_porch: 148,
            v_active: 1080,
            v_front_porch: 4,
            v_sync_width: 5,
            v_back_porch: 36,
            interlaced: false,
            polarity: DisplaySyncPolarity {
                hsync_positive: true,
                vsync_positive: true,
            },
        },
        95 => DisplayTiming {
            pixel_clock_khz: 297_000,
            h_active: 3840,
            h_front_porch: 176,
            h_sync_width: 88,
            h_back_porch: 296,
            v_active: 2160,
            v_front_porch: 8,
            v_sync_width: 10,
            v_back_porch: 72,
            interlaced: false,
            polarity: DisplaySyncPolarity {
                hsync_positive: true,
                vsync_positive: true,
            },
        },
        97 => DisplayTiming {
            pixel_clock_khz: 594_000,
            h_active: 3840,
            h_front_porch: 176,
            h_sync_width: 88,
            h_back_porch: 296,
            v_active: 2160,
            v_front_porch: 8,
            v_sync_width: 10,
            v_back_porch: 72,
            interlaced: false,
            polarity: DisplaySyncPolarity {
                hsync_positive: true,
                vsync_positive: true,
            },
        },
        _ => return None,
    };

    Some((mode_from_timing(timing, false), timing))
}

pub(crate) fn mode_from_timing(timing: DisplayTiming, preferred: bool) -> DisplayMode {
    let total_pixels = u64::from(timing.horizontal_total()) * u64::from(timing.vertical_total());
    let refresh_hz_milli = if total_pixels == 0 {
        0
    } else {
        ((u64::from(timing.pixel_clock_khz) * 1_000_000) / total_pixels) as u32
    };

    DisplayMode {
        width: timing.h_active,
        height: timing.v_active,
        refresh_hz_milli,
        interlaced: timing.interlaced,
        preferred,
    }
}

fn has_preferred_mode(sink: &ParsedEdidSink) -> bool {
    sink.modes[..sink.mode_count]
        .iter()
        .any(|mode| mode.preferred)
}

pub(crate) fn same_mode(lhs: DisplayMode, rhs: DisplayMode) -> bool {
    lhs.width == rhs.width
        && lhs.height == rhs.height
        && lhs.refresh_hz_milli == rhs.refresh_hz_milli
        && lhs.interlaced == rhs.interlaced
}

pub(crate) fn mode_within_port_caps(mode: DisplayMode, caps: DisplayPortCapabilities) -> bool {
    mode.width <= caps.max_width
        && mode.height <= caps.max_height
        && mode.refresh_hz_milli <= caps.max_refresh_hz.saturating_mul(1000)
}

pub(crate) fn select_pixel_format(
    requested: DisplayPixelFormatSupport,
    port: DisplayPixelFormatSupport,
    sink: DisplayPixelFormatSupport,
) -> Option<DisplayPixelFormat> {
    let preferences = [
        DisplayPixelFormat::Argb8888,
        DisplayPixelFormat::Xrgb8888,
        DisplayPixelFormat::Abgr8888,
        DisplayPixelFormat::Xbgr8888,
        DisplayPixelFormat::Rgb888,
        DisplayPixelFormat::Bgr888,
        DisplayPixelFormat::Rgb565,
    ];

    for format in preferences {
        if requested.supports(format) && port.supports(format) && sink.supports(format) {
            return Some(format);
        }
    }

    for format in preferences {
        if port.supports(format) && sink.supports(format) {
            return Some(format);
        }
    }

    None
}

pub(crate) fn select_color_space(
    requested: DisplayColorSpaceSupport,
    sink: DisplayColorSpaceSupport,
) -> DisplayColorSpace {
    let preferences = [
        DisplayColorSpace::Rgb,
        DisplayColorSpace::Ycbcr444,
        DisplayColorSpace::Ycbcr422,
        DisplayColorSpace::Ycbcr420,
    ];

    for color_space in preferences {
        if requested.supports(color_space) && sink.supports(color_space) {
            return color_space;
        }
    }

    for color_space in preferences {
        if sink.supports(color_space) {
            return color_space;
        }
    }

    DisplayColorSpace::Rgb
}

pub(crate) fn select_quantization(
    requested: DisplayQuantizationSupport,
    sink: DisplayQuantizationSupport,
) -> DisplayQuantization {
    let preferences = [
        DisplayQuantization::Default,
        DisplayQuantization::Full,
        DisplayQuantization::Limited,
    ];

    for quantization in preferences {
        if requested.supports(quantization) && sink.supports(quantization) {
            return quantization;
        }
    }

    for quantization in preferences {
        if sink.supports(quantization) {
            return quantization;
        }
    }

    DisplayQuantization::Default
}

pub(crate) fn contains_mode(modes: &[DisplayMode], candidate: DisplayMode) -> bool {
    modes.iter().copied().any(|mode| same_mode(mode, candidate))
}

pub(crate) fn matches_requested_color_space(
    requested: DisplayColorSpaceSupport,
    selected: DisplayColorSpace,
) -> bool {
    requested.supports(selected)
}

pub(crate) fn matches_requested_quantization(
    requested: DisplayQuantizationSupport,
    selected: DisplayQuantization,
) -> bool {
    requested.supports(selected)
}
