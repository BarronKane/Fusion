//! DisplayPort display endpoint driver family.

#![cfg_attr(not(feature = "std"), no_std)]

use core::marker::PhantomData;

use fusion_hal::drivers::display::shared::edid::{
    contains_mode,
    matches_requested_color_space,
    matches_requested_quantization,
    mode_within_port_caps,
    parse_edid_sink,
    select_color_space,
    select_pixel_format,
    select_quantization,
    ParsedEdidSink,
};
use fusion_hal::drivers::display::shared::support::{
    map_config_error,
    map_display_error,
};
use fusion_hal::contract::drivers::display::{
    DisplayActiveConfig,
    DisplayConfigError,
    DisplayConnectorKind,
    DisplayControlContract,
    DisplayControlState,
    DisplayDescriptorSet,
    DisplayError,
    DisplayFeature,
    DisplayFeatureCapabilities,
    DisplayFeatureValue,
    DisplayFrameView,
    DisplayHotplugEvent,
    DisplayIdentity,
    DisplayMode,
    DisplayNegotiationReason,
    DisplayNegotiationRequest,
    DisplayNegotiationResult,
    DisplayOutputDescriptor,
    DisplayPortCapabilities,
    DisplayPortContract,
    DisplayPortDescriptor,
    DisplayPortState,
    DisplayPowerState,
    DisplayPresentReport,
    DisplayPresentRequest,
    DisplayRawDescriptorKind,
    DisplayRegion,
    DisplayResult,
    DisplaySinkCapabilities,
    DisplaySurfaceBinding,
    DisplayTiming,
    DisplayUploadReport,
};
use fusion_hal::contract::drivers::driver::{
    ActiveDriver,
    DriverActivation,
    DriverActivationContext,
    DriverBindingSource,
    DriverClass,
    DriverContract,
    DriverDiscoveryContext,
    DriverError,
    DriverIdentity,
    DriverMetadata,
    DriverRegistration,
    RegisteredDriver,
};
pub(crate) use fusion_hal::contract::drivers::driver::{
    DriverContractKey,
    DriverDogma,
    DriverUsefulness,
};

mod dogma;
#[cfg(any(target_os = "none", feature = "fdxe-module"))]
mod fdxe;
#[path = "interface/interface.rs"]
pub mod interface;
mod unsupported;

#[cfg(test)]
use fusion_hal::drivers::display::shared::edid::EDID_BLOCK_BYTES;
#[cfg(test)]
use fusion_hal::contract::drivers::display::{
    DisplayColorSpace,
    DisplayColorSpaceSupport,
    DisplayPixelFormat,
    DisplayPixelFormatSupport,
    DisplayQuantizationSupport,
};

const DISPLAY_PORT_DRIVER_BINDING_SOURCES: [DriverBindingSource; 6] = [
    DriverBindingSource::StaticSoc,
    DriverBindingSource::BoardManifest,
    DriverBindingSource::Acpi,
    DriverBindingSource::Devicetree,
    DriverBindingSource::Pci,
    DriverBindingSource::Manual,
];
const DISPLAY_PORT_DRIVER_METADATA: DriverMetadata = DriverMetadata {
    key: dogma::DISPLAY_PORT_DRIVER_DOGMA.key,
    class: DriverClass::Display,
    identity: DriverIdentity {
        vendor: "Fusion",
        family: Some("Display"),
        package: None,
        product: "DisplayPort driver",
        advertised_interface: "DisplayPort endpoint",
    },
    contracts: dogma::DISPLAY_PORT_DRIVER_DOGMA.contracts,
    required_contracts: dogma::DISPLAY_PORT_DRIVER_DOGMA.required_contracts,
    usefulness: dogma::DISPLAY_PORT_DRIVER_DOGMA.usefulness,
    singleton_class: dogma::DISPLAY_PORT_DRIVER_DOGMA.singleton_class,
    binding_sources: &DISPLAY_PORT_DRIVER_BINDING_SOURCES,
    description: "DisplayPort endpoint driver layered over one selected DisplayPort hardware substrate",
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayPortBinding {
    pub provider: u8,
    pub output_name: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DisplayPortRuntimeState {
    pub enabled: bool,
    pub blanked: bool,
    pub active_config: Option<DisplayActiveConfig>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
struct DisplayPortTransportCaps {
    dpcd_revision_major: u8,
    dpcd_revision_minor: u8,
    max_link_rate_kbps: Option<u32>,
    max_lane_count: u8,
    enhanced_framing: bool,
    downspread: bool,
    has_display_id: bool,
}

/// Hardware-facing DisplayPort seam consumed by the DisplayPort driver family.
///
/// The public DisplayPort driver owns descriptor parsing, sink identity, capability derivation,
/// negotiation, and config validation. The backend only needs to surface raw descriptors,
/// connector truth, and the actual output programming/presentation operations.
pub trait DisplayPortHardware {
    fn provider_count() -> u8;
    fn output_descriptor(provider: u8) -> Option<&'static DisplayOutputDescriptor>;

    fn connected(provider: u8) -> DisplayResult<bool>;
    fn raw_descriptors(provider: u8) -> DisplayResult<DisplayDescriptorSet<'static>>;

    fn feature_capabilities(_provider: u8) -> DisplayResult<DisplayFeatureCapabilities> {
        Ok(DisplayFeatureCapabilities::default())
    }

    fn get_feature(_provider: u8, _feature: DisplayFeature) -> DisplayResult<DisplayFeatureValue> {
        Err(DisplayError::unsupported())
    }

    fn set_feature(
        _provider: u8,
        _feature: DisplayFeature,
        _value: DisplayFeatureValue,
    ) -> DisplayResult<()> {
        Err(DisplayError::unsupported())
    }

    fn power_state(provider: u8) -> DisplayResult<DisplayPowerState> {
        if Self::connected(provider)? {
            Ok(DisplayPowerState::On)
        } else {
            Ok(DisplayPowerState::Off)
        }
    }

    fn set_power_state(_provider: u8, _state: DisplayPowerState) -> DisplayResult<()> {
        Err(DisplayError::unsupported())
    }

    fn port_descriptor(provider: u8) -> DisplayResult<DisplayPortDescriptor>;
    fn port_capabilities(provider: u8) -> DisplayResult<DisplayPortCapabilities>;
    fn runtime_state(provider: u8) -> DisplayResult<DisplayPortRuntimeState>;
    fn set_config(provider: u8, config: &DisplayActiveConfig) -> DisplayResult<()>;
    fn enable(provider: u8) -> DisplayResult<()>;
    fn disable(provider: u8) -> DisplayResult<()>;
    fn blank(provider: u8, blanked: bool) -> DisplayResult<()>;
    fn attach_surface(provider: u8, surface: DisplaySurfaceBinding) -> DisplayResult<()>;
    fn detach_surface(provider: u8) -> DisplayResult<()>;
    fn upload_frame(
        provider: u8,
        frame: &DisplayFrameView<'_>,
        region: Option<DisplayRegion>,
    ) -> DisplayResult<DisplayUploadReport>;
    fn present(
        provider: u8,
        request: &DisplayPresentRequest,
    ) -> DisplayResult<DisplayPresentReport>;
    fn flush(provider: u8) -> DisplayResult<()>;
    fn wait_vblank(provider: u8, timeout_ms: u32) -> DisplayResult<()>;
    fn wait_hotplug_event(
        provider: u8,
        timeout_ms: u32,
    ) -> DisplayResult<Option<DisplayHotplugEvent>>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedDisplayPortHardware;

impl DisplayPortHardware for UnsupportedDisplayPortHardware {
    fn provider_count() -> u8 {
        0
    }

    fn output_descriptor(_provider: u8) -> Option<&'static DisplayOutputDescriptor> {
        None
    }

    fn connected(_provider: u8) -> DisplayResult<bool> {
        Ok(false)
    }

    fn raw_descriptors(_provider: u8) -> DisplayResult<DisplayDescriptorSet<'static>> {
        Ok(DisplayDescriptorSet::default())
    }

    fn port_descriptor(_provider: u8) -> DisplayResult<DisplayPortDescriptor> {
        Err(DisplayError::unsupported())
    }

    fn port_capabilities(_provider: u8) -> DisplayResult<DisplayPortCapabilities> {
        Err(DisplayError::unsupported())
    }

    fn runtime_state(_provider: u8) -> DisplayResult<DisplayPortRuntimeState> {
        Err(DisplayError::unsupported())
    }

    fn set_config(_provider: u8, _config: &DisplayActiveConfig) -> DisplayResult<()> {
        Err(DisplayError::unsupported())
    }

    fn enable(_provider: u8) -> DisplayResult<()> {
        Err(DisplayError::unsupported())
    }

    fn disable(_provider: u8) -> DisplayResult<()> {
        Err(DisplayError::unsupported())
    }

    fn blank(_provider: u8, _blanked: bool) -> DisplayResult<()> {
        Err(DisplayError::unsupported())
    }

    fn attach_surface(_provider: u8, _surface: DisplaySurfaceBinding) -> DisplayResult<()> {
        Err(DisplayError::unsupported())
    }

    fn detach_surface(_provider: u8) -> DisplayResult<()> {
        Err(DisplayError::unsupported())
    }

    fn upload_frame(
        _provider: u8,
        _frame: &DisplayFrameView<'_>,
        _region: Option<DisplayRegion>,
    ) -> DisplayResult<DisplayUploadReport> {
        Err(DisplayError::unsupported())
    }

    fn present(
        _provider: u8,
        _request: &DisplayPresentRequest,
    ) -> DisplayResult<DisplayPresentReport> {
        Err(DisplayError::unsupported())
    }

    fn flush(_provider: u8) -> DisplayResult<()> {
        Err(DisplayError::unsupported())
    }

    fn wait_vblank(_provider: u8, _timeout_ms: u32) -> DisplayResult<()> {
        Err(DisplayError::unsupported())
    }

    fn wait_hotplug_event(
        _provider: u8,
        _timeout_ms: u32,
    ) -> DisplayResult<Option<DisplayHotplugEvent>> {
        Err(DisplayError::unsupported())
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DisplayPortDriver<H: DisplayPortHardware = unsupported::UnsupportedDisplayPortHardware> {
    marker: PhantomData<fn() -> H>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DisplayPortDriverContext<
    H: DisplayPortHardware = unsupported::UnsupportedDisplayPortHardware,
> {
    marker: PhantomData<fn() -> H>,
}

impl<H> DisplayPortDriverContext<H>
where
    H: DisplayPortHardware,
{
    #[must_use]
    pub const fn new() -> Self {
        Self {
            marker: PhantomData,
        }
    }
}

#[must_use]
pub const fn driver_metadata() -> &'static DriverMetadata {
    &DISPLAY_PORT_DRIVER_METADATA
}

type DisplayPortSinkCache = ParsedEdidSink;

#[derive(Debug, Clone)]
pub struct DisplayPortEndpoint<H: DisplayPortHardware = unsupported::UnsupportedDisplayPortHardware>
{
    provider: u8,
    sink: DisplayPortSinkCache,
    transport: DisplayPortTransportCaps,
    _hardware: PhantomData<H>,
}

impl<H> DisplayPortEndpoint<H>
where
    H: DisplayPortHardware,
{
    fn try_new(provider: u8) -> DisplayResult<Self> {
        let descriptors = H::raw_descriptors(provider)?;
        let sink = parse_edid_sink(DisplayConnectorKind::DisplayPort, descriptors);
        let transport = parse_display_port_transport(descriptors);
        Ok(Self {
            provider,
            sink,
            transport,
            _hardware: PhantomData,
        })
    }

    fn refresh_sink(&mut self) -> DisplayResult<()> {
        let descriptors = H::raw_descriptors(self.provider)?;
        self.sink = parse_edid_sink(DisplayConnectorKind::DisplayPort, descriptors);
        self.transport = parse_display_port_transport(descriptors);
        Ok(())
    }

    fn negotiate_config(
        &self,
        request: &DisplayNegotiationRequest<'_>,
    ) -> DisplayResult<DisplayNegotiationResult> {
        if self.sink.mode_count == 0 {
            return Err(DisplayError::negotiation_failed());
        }

        let port_caps = H::port_capabilities(self.provider)?;
        let selected_mode = self.select_mode(request, &port_caps)?;
        let timing = self
            .sink
            .timing_for_mode(selected_mode)
            .ok_or_else(DisplayError::negotiation_failed)?;
        let pixel_format = select_pixel_format(
            request.preferred_pixel_formats,
            port_caps.supported_pixel_formats,
            self.sink.pixel_formats,
        )
        .ok_or_else(DisplayError::negotiation_failed)?;
        let color_space =
            select_color_space(request.preferred_color_spaces, self.sink.color_spaces);
        let quantization =
            select_quantization(request.preferred_quantization, self.sink.quantization);

        let audio_enabled = if request.require_audio {
            if self.sink.audio.basic_pcm || self.sink.audio.max_channels > 0 {
                true
            } else {
                return Err(DisplayError::negotiation_failed());
            }
        } else {
            false
        };

        let hdr_enabled = request.prefer_hdr
            && (self.sink.hdr.hdr_static_metadata
                || self.sink.hdr.hdr10
                || self.sink.hdr.hybrid_log_gamma);
        let vrr_enabled = request.prefer_vrr && self.sink.vrr.adaptive_sync;

        let reason = if contains_mode(request.preferred_modes, selected_mode)
            && matches_requested_color_space(request.preferred_color_spaces, color_space)
            && matches_requested_quantization(request.preferred_quantization, quantization)
            && (!request.require_audio || audio_enabled)
        {
            DisplayNegotiationReason::Requested
        } else if selected_mode.preferred {
            DisplayNegotiationReason::SafeFallback
        } else {
            DisplayNegotiationReason::ClosestMatch
        };

        Ok(DisplayNegotiationResult {
            config: DisplayActiveConfig {
                mode: selected_mode,
                timing,
                pixel_format,
                color_space,
                quantization,
                audio_enabled,
                hdr_enabled,
                vrr_enabled,
            },
            reason,
        })
    }

    fn select_mode(
        &self,
        request: &DisplayNegotiationRequest<'_>,
        port_caps: &DisplayPortCapabilities,
    ) -> DisplayResult<DisplayMode> {
        for requested in request.preferred_modes {
            if self.sink.supports_mode(*requested) && mode_within_port_caps(*requested, *port_caps)
            {
                return Ok(*requested);
            }
        }

        if let Some(preferred) = self.sink.modes[..self.sink.mode_count]
            .iter()
            .copied()
            .find(|mode| mode.preferred && mode_within_port_caps(*mode, *port_caps))
        {
            return Ok(preferred);
        }

        self.sink.modes[..self.sink.mode_count]
            .iter()
            .copied()
            .find(|mode| mode_within_port_caps(*mode, *port_caps))
            .ok_or_else(DisplayError::negotiation_failed)
    }

    fn validate_active_config(
        &self,
        config: &DisplayActiveConfig,
    ) -> Result<(), DisplayConfigError> {
        let port_caps =
            H::port_capabilities(self.provider).map_err(|_| DisplayConfigError::NotReady)?;
        if !self.sink.supports_mode(config.mode) {
            return Err(DisplayConfigError::UnsupportedMode);
        }
        let expected_timing = self
            .sink
            .timing_for_mode(config.mode)
            .ok_or(DisplayConfigError::UnsupportedTiming)?;
        if config.timing != expected_timing {
            return Err(DisplayConfigError::UnsupportedTiming);
        }
        if !port_caps
            .supported_pixel_formats
            .supports(config.pixel_format)
            || !self.sink.pixel_formats.supports(config.pixel_format)
        {
            return Err(DisplayConfigError::UnsupportedPixelFormat);
        }
        if !self.sink.color_spaces.supports(config.color_space) {
            return Err(DisplayConfigError::UnsupportedColorSpace);
        }
        if !self.sink.quantization.supports(config.quantization) {
            return Err(DisplayConfigError::UnsupportedQuantization);
        }
        if config.audio_enabled && !(self.sink.audio.basic_pcm || self.sink.audio.max_channels > 0)
        {
            return Err(DisplayConfigError::UnsupportedMode);
        }
        if config.hdr_enabled
            && !(self.sink.hdr.hdr_static_metadata
                || self.sink.hdr.hdr10
                || self.sink.hdr.hybrid_log_gamma)
        {
            return Err(DisplayConfigError::UnsupportedMode);
        }
        if config.vrr_enabled && !self.sink.vrr.adaptive_sync {
            return Err(DisplayConfigError::UnsupportedMode);
        }
        if !mode_within_port_caps(config.mode, port_caps) {
            return Err(DisplayConfigError::BandwidthExceeded);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DisplayPortEndpointPort<
    'a,
    H: DisplayPortHardware = unsupported::UnsupportedDisplayPortHardware,
> {
    endpoint: &'a DisplayPortEndpoint<H>,
}

impl<'a, H> DisplayPortEndpointPort<'a, H>
where
    H: DisplayPortHardware,
{
    fn new(endpoint: &'a DisplayPortEndpoint<H>) -> Self {
        Self { endpoint }
    }

    fn provider(&self) -> u8 {
        self.endpoint.provider
    }
}

impl<H> DisplayControlContract for DisplayPortEndpoint<H>
where
    H: DisplayPortHardware,
{
    type Port<'a>
        = DisplayPortEndpointPort<'a, H>
    where
        Self: 'a;

    fn state(&self) -> DisplayResult<DisplayControlState> {
        Ok(DisplayControlState {
            connected: H::connected(self.provider)?,
            descriptor_valid: self.sink.descriptor_valid,
            sink_power: H::power_state(self.provider)?,
        })
    }

    fn refresh(&mut self) -> DisplayResult<()> {
        self.refresh_sink()
    }

    fn identify(&self) -> DisplayResult<DisplayIdentity> {
        Ok(self.sink.identity)
    }

    fn sink_capabilities(&self) -> DisplayResult<DisplaySinkCapabilities<'_>> {
        if !self.sink.descriptor_valid {
            return Err(DisplayError::invalid());
        }
        Ok(self.sink.sink_capabilities())
    }

    fn raw_descriptors(&self) -> DisplayResult<DisplayDescriptorSet<'_>> {
        Ok(self.sink.descriptors)
    }

    fn negotiate(
        &self,
        request: &DisplayNegotiationRequest<'_>,
    ) -> DisplayResult<DisplayNegotiationResult> {
        self.negotiate_config(request)
    }

    fn feature_capabilities(&self) -> DisplayResult<DisplayFeatureCapabilities> {
        H::feature_capabilities(self.provider)
    }

    fn get_feature(&self, feature: DisplayFeature) -> DisplayResult<DisplayFeatureValue> {
        H::get_feature(self.provider, feature)
    }

    fn set_feature(
        &mut self,
        feature: DisplayFeature,
        value: DisplayFeatureValue,
    ) -> DisplayResult<()> {
        H::set_feature(self.provider, feature, value)
    }

    fn power_state(&self) -> DisplayResult<DisplayPowerState> {
        H::power_state(self.provider)
    }

    fn set_power_state(&mut self, state: DisplayPowerState) -> DisplayResult<()> {
        H::set_power_state(self.provider, state)
    }

    fn port(&self) -> DisplayResult<Self::Port<'_>> {
        Ok(DisplayPortEndpointPort::new(self))
    }

    fn port_mut(&mut self) -> DisplayResult<Self::Port<'_>> {
        Ok(DisplayPortEndpointPort::new(self))
    }
}

impl<'a, H> DisplayPortContract for DisplayPortEndpointPort<'a, H>
where
    H: DisplayPortHardware,
{
    fn descriptor(&self) -> DisplayResult<DisplayPortDescriptor> {
        H::port_descriptor(self.provider())
    }

    fn state(&self) -> DisplayResult<DisplayPortState> {
        let runtime = H::runtime_state(self.provider())?;
        Ok(DisplayPortState {
            connected: H::connected(self.provider())?,
            enabled: runtime.enabled,
            blanked: runtime.blanked,
            configured: runtime.active_config.is_some(),
            active_config: runtime.active_config,
        })
    }

    fn capabilities(&self) -> DisplayResult<DisplayPortCapabilities> {
        H::port_capabilities(self.provider())
    }

    fn validate_config(&self, config: &DisplayActiveConfig) -> Result<(), DisplayConfigError> {
        self.endpoint.validate_active_config(config)
    }

    fn active_config(&self) -> DisplayResult<Option<DisplayActiveConfig>> {
        Ok(H::runtime_state(self.provider())?.active_config)
    }

    fn set_config(&mut self, config: &DisplayActiveConfig) -> DisplayResult<()> {
        self.validate_config(config).map_err(map_config_error)?;
        H::set_config(self.provider(), config)
    }

    fn enable(&mut self) -> DisplayResult<()> {
        H::enable(self.provider())
    }

    fn disable(&mut self) -> DisplayResult<()> {
        H::disable(self.provider())
    }

    fn blank(&mut self, blanked: bool) -> DisplayResult<()> {
        H::blank(self.provider(), blanked)
    }

    fn attach_surface(&mut self, surface: DisplaySurfaceBinding) -> DisplayResult<()> {
        H::attach_surface(self.provider(), surface)
    }

    fn detach_surface(&mut self) -> DisplayResult<()> {
        H::detach_surface(self.provider())
    }

    fn upload_frame(
        &mut self,
        frame: &DisplayFrameView<'_>,
        region: Option<DisplayRegion>,
    ) -> DisplayResult<DisplayUploadReport> {
        H::upload_frame(self.provider(), frame, region)
    }

    fn present(&mut self, request: &DisplayPresentRequest) -> DisplayResult<DisplayPresentReport> {
        H::present(self.provider(), request)
    }

    fn flush(&mut self) -> DisplayResult<()> {
        H::flush(self.provider())
    }

    fn wait_vblank(&mut self, timeout_ms: u32) -> DisplayResult<()> {
        H::wait_vblank(self.provider(), timeout_ms)
    }

    fn wait_hotplug_event(
        &mut self,
        timeout_ms: u32,
    ) -> DisplayResult<Option<DisplayHotplugEvent>> {
        H::wait_hotplug_event(self.provider(), timeout_ms)
    }

    fn timing(&self) -> DisplayResult<Option<DisplayTiming>> {
        Ok(H::runtime_state(self.provider())?
            .active_config
            .map(|config| config.timing))
    }
}

fn enumerate_display_port_bindings<H>(
    _registered: &RegisteredDriver<DisplayPortDriver<H>>,
    context: &mut DriverDiscoveryContext<'_>,
    out: &mut [DisplayPortBinding],
) -> Result<usize, DriverError>
where
    H: DisplayPortHardware + 'static,
{
    let _ = context.downcast_mut::<DisplayPortDriverContext<H>>()?;
    if out.is_empty() {
        return Err(DriverError::resource_exhausted());
    }

    let mut written = 0;
    for provider in 0..H::provider_count() {
        if written == out.len() {
            return Err(DriverError::resource_exhausted());
        }
        let Some(descriptor) = H::output_descriptor(provider) else {
            continue;
        };
        if descriptor.connector != DisplayConnectorKind::DisplayPort {
            continue;
        }
        out[written] = DisplayPortBinding {
            provider,
            output_name: descriptor.name,
        };
        written += 1;
    }

    Ok(written)
}

fn activate_display_port_binding<H>(
    _registered: &RegisteredDriver<DisplayPortDriver<H>>,
    context: &mut DriverActivationContext<'_>,
    binding: DisplayPortBinding,
) -> Result<ActiveDriver<DisplayPortDriver<H>>, DriverError>
where
    H: DisplayPortHardware + 'static,
{
    let _ = context.downcast_mut::<DisplayPortDriverContext<H>>()?;
    let Some(descriptor) = H::output_descriptor(binding.provider) else {
        return Err(DriverError::invalid());
    };
    if descriptor.name != binding.output_name {
        return Err(DriverError::invalid());
    }

    let instance =
        DisplayPortEndpoint::<H>::try_new(binding.provider).map_err(map_display_error)?;
    Ok(ActiveDriver::new(binding, instance))
}

impl<H> DriverContract for DisplayPortDriver<H>
where
    H: DisplayPortHardware + 'static,
{
    type Binding = DisplayPortBinding;
    type Instance = DisplayPortEndpoint<H>;

    fn registration() -> DriverRegistration<Self> {
        DriverRegistration::new(
            driver_metadata,
            DriverActivation::new(
                enumerate_display_port_bindings::<H>,
                activate_display_port_binding::<H>,
            ),
        )
    }
}

fn find_descriptor_bytes(
    descriptors: DisplayDescriptorSet<'static>,
    kind: DisplayRawDescriptorKind,
) -> Option<&'static [u8]> {
    descriptors
        .descriptors
        .iter()
        .find(|descriptor| descriptor.kind == kind)
        .map(|descriptor| descriptor.bytes)
}

fn parse_display_port_transport(
    descriptors: DisplayDescriptorSet<'static>,
) -> DisplayPortTransportCaps {
    let mut transport = DisplayPortTransportCaps {
        has_display_id: find_descriptor_bytes(descriptors, DisplayRawDescriptorKind::DisplayId)
            .is_some(),
        ..DisplayPortTransportCaps::default()
    };

    let Some(dpcd) = find_descriptor_bytes(descriptors, DisplayRawDescriptorKind::Dpcd) else {
        return transport;
    };
    if dpcd.len() < 3 {
        return transport;
    }

    let revision = dpcd[0];
    transport.dpcd_revision_major = revision >> 4;
    transport.dpcd_revision_minor = revision & 0x0f;
    transport.max_link_rate_kbps = dpcd_link_rate_kbps(dpcd[1]);
    transport.max_lane_count = dpcd[2] & 0x1f;
    transport.enhanced_framing = dpcd[2] & 0x80 != 0;
    if dpcd.len() > 3 {
        transport.downspread = dpcd[3] & 0x01 != 0;
    }

    transport
}

fn dpcd_link_rate_kbps(code: u8) -> Option<u32> {
    match code {
        0x06 => Some(162_000),
        0x0a => Some(270_000),
        0x14 => Some(540_000),
        0x1e => Some(810_000),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_OUTPUT_DESCRIPTOR: DisplayOutputDescriptor = DisplayOutputDescriptor {
        id: fusion_hal::contract::drivers::display::DisplayOutputId(0),
        name: "dp-0",
        connector: DisplayConnectorKind::DisplayPort,
        hotplug_supported: true,
    };

    fn build_base_edid() -> [u8; EDID_BLOCK_BYTES] {
        let mut edid = [0u8; EDID_BLOCK_BYTES];
        edid[..8].copy_from_slice(&[0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00]);
        edid[8] = 0x04;
        edid[9] = 0x6d;
        edid[10] = 0xcd;
        edid[11] = 0xab;
        edid[12] = 0x12;
        edid[13] = 0x34;
        edid[14] = 0x56;
        edid[15] = 0x78;

        let dtd = &mut edid[54..72];
        dtd.copy_from_slice(&[
            0x02, 0x3a, 0x80, 0x18, 0x71, 0x38, 0x2d, 0x40, 0x58, 0x2c, 0x45, 0x00, 0xa0, 0x5a,
            0x00, 0x00, 0x00, 0x1e,
        ]);

        let name_descriptor = &mut edid[72..90];
        name_descriptor.copy_from_slice(&[
            0x00, 0x00, 0x00, 0xfc, 0x00, b'F', b'u', b's', b'i', b'o', b'n', b' ', b'D', b'P',
            0x0a, 0x20, 0x20, 0x20,
        ]);

        edid[126] = 1;
        finalize_edid_checksum(&mut edid);
        edid
    }

    fn build_cta_extension() -> [u8; EDID_BLOCK_BYTES] {
        let mut extension = [0u8; EDID_BLOCK_BYTES];
        extension[0] = 0x02;
        extension[1] = 0x03;
        extension[2] = 0x0f;
        extension[3] = 0x70;
        extension[4] = 0x23;
        extension[5] = 0x09;
        extension[6] = 0x07;
        extension[7] = 0x07;
        extension[8] = 0x42;
        extension[9] = 0x90;
        extension[10] = 0x04;
        extension[11] = 0xe3;
        extension[12] = 0x06;
        extension[13] = 0x0c;
        extension[14] = 0x00;
        finalize_edid_checksum(&mut extension);
        extension
    }

    fn build_dpcd() -> [u8; 16] {
        let mut dpcd = [0u8; 16];
        dpcd[0] = 0x14;
        dpcd[1] = 0x1e;
        dpcd[2] = 0x84;
        dpcd[3] = 0x01;
        dpcd
    }

    fn build_display_id() -> [u8; 8] {
        [0x70, 0x01, 0x01, 0x00, 0, 0, 0, 0]
    }

    fn finalize_edid_checksum(block: &mut [u8; EDID_BLOCK_BYTES]) {
        let checksum = block[..EDID_BLOCK_BYTES - 1]
            .iter()
            .fold(0u8, |sum, byte| sum.wrapping_add(*byte));
        block[EDID_BLOCK_BYTES - 1] = checksum.wrapping_neg();
    }

    struct FakeDisplayPortHardware;

    impl DisplayPortHardware for FakeDisplayPortHardware {
        fn provider_count() -> u8 {
            1
        }

        fn output_descriptor(provider: u8) -> Option<&'static DisplayOutputDescriptor> {
            if provider == 0 {
                Some(&TEST_OUTPUT_DESCRIPTOR)
            } else {
                None
            }
        }

        fn connected(_provider: u8) -> DisplayResult<bool> {
            Ok(true)
        }

        fn raw_descriptors(_provider: u8) -> DisplayResult<DisplayDescriptorSet<'static>> {
            let mut edid = build_base_edid().to_vec();
            edid.extend_from_slice(&build_cta_extension());
            let edid = Box::leak(edid.into_boxed_slice());
            let dpcd = Box::leak(Box::new(build_dpcd())).as_slice();
            let display_id = Box::leak(Box::new(build_display_id())).as_slice();
            let descriptors = Box::leak(
                vec![
                    fusion_hal::contract::drivers::display::DisplayRawDescriptor {
                        kind: DisplayRawDescriptorKind::Edid,
                        bytes: edid,
                    },
                    fusion_hal::contract::drivers::display::DisplayRawDescriptor {
                        kind: DisplayRawDescriptorKind::Dpcd,
                        bytes: dpcd,
                    },
                    fusion_hal::contract::drivers::display::DisplayRawDescriptor {
                        kind: DisplayRawDescriptorKind::DisplayId,
                        bytes: display_id,
                    },
                ]
                .into_boxed_slice(),
            );
            Ok(DisplayDescriptorSet { descriptors })
        }

        fn port_descriptor(_provider: u8) -> DisplayResult<DisplayPortDescriptor> {
            Ok(DisplayPortDescriptor {
                connector: DisplayConnectorKind::DisplayPort,
                hotplug_supported: true,
                hotplug_event_supported: true,
                cpu_upload_supported: true,
                direct_scanout_supported: true,
                page_flip_supported: true,
                partial_update_supported: true,
                vblank_wait_supported: true,
            })
        }

        fn port_capabilities(_provider: u8) -> DisplayResult<DisplayPortCapabilities> {
            Ok(DisplayPortCapabilities {
                max_width: 3840,
                max_height: 2160,
                max_refresh_hz: 60,
                supported_pixel_formats: DisplayPixelFormatSupport {
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
                min_stride_alignment: 4,
                min_surface_alignment: 4,
            })
        }

        fn runtime_state(_provider: u8) -> DisplayResult<DisplayPortRuntimeState> {
            Ok(DisplayPortRuntimeState {
                enabled: true,
                blanked: false,
                active_config: None,
            })
        }

        fn set_config(_provider: u8, _config: &DisplayActiveConfig) -> DisplayResult<()> {
            Ok(())
        }

        fn enable(_provider: u8) -> DisplayResult<()> {
            Ok(())
        }

        fn disable(_provider: u8) -> DisplayResult<()> {
            Ok(())
        }

        fn blank(_provider: u8, _blanked: bool) -> DisplayResult<()> {
            Ok(())
        }

        fn attach_surface(_provider: u8, _surface: DisplaySurfaceBinding) -> DisplayResult<()> {
            Ok(())
        }

        fn detach_surface(_provider: u8) -> DisplayResult<()> {
            Ok(())
        }

        fn upload_frame(
            _provider: u8,
            _frame: &DisplayFrameView<'_>,
            _region: Option<DisplayRegion>,
        ) -> DisplayResult<DisplayUploadReport> {
            Ok(DisplayUploadReport {
                bytes_uploaded: 0,
                region_applied: None,
            })
        }

        fn present(
            _provider: u8,
            _request: &DisplayPresentRequest,
        ) -> DisplayResult<DisplayPresentReport> {
            Ok(DisplayPresentReport {
                presented: true,
                frame_id: fusion_hal::contract::drivers::display::DisplayFrameId(1),
                vblank_sequence: Some(1),
            })
        }

        fn flush(_provider: u8) -> DisplayResult<()> {
            Ok(())
        }

        fn wait_vblank(_provider: u8, _timeout_ms: u32) -> DisplayResult<()> {
            Ok(())
        }

        fn wait_hotplug_event(
            _provider: u8,
            _timeout_ms: u32,
        ) -> DisplayResult<Option<DisplayHotplugEvent>> {
            Ok(None)
        }
    }

    #[test]
    fn display_port_parser_extracts_identity_and_transport() {
        let endpoint = DisplayPortEndpoint::<FakeDisplayPortHardware>::try_new(0).unwrap();
        assert!(endpoint.sink.descriptor_valid);
        assert_eq!(endpoint.sink.identity.product_code, Some(0xabcd));
        assert_eq!(
            endpoint
                .sink
                .identity
                .model_name
                .map(|text| text.as_bytes().to_vec()),
            Some(b"Fusion DP".to_vec())
        );
        assert!(endpoint.sink.mode_count >= 2);
        assert_eq!(endpoint.transport.dpcd_revision_major, 1);
        assert_eq!(endpoint.transport.dpcd_revision_minor, 4);
        assert_eq!(endpoint.transport.max_link_rate_kbps, Some(810_000));
        assert_eq!(endpoint.transport.max_lane_count, 4);
        assert!(endpoint.transport.enhanced_framing);
        assert!(endpoint.transport.downspread);
        assert!(endpoint.transport.has_display_id);
    }

    #[test]
    fn display_port_driver_negotiates_requested_mode() {
        let endpoint = DisplayPortEndpoint::<FakeDisplayPortHardware>::try_new(0).unwrap();
        let result = endpoint
            .negotiate(&DisplayNegotiationRequest {
                preferred_modes: &[DisplayMode {
                    width: 1920,
                    height: 1080,
                    refresh_hz_milli: 60_000,
                    interlaced: false,
                    preferred: false,
                }],
                preferred_pixel_formats: DisplayPixelFormatSupport {
                    xrgb8888: true,
                    ..DisplayPixelFormatSupport::default()
                },
                preferred_color_spaces: DisplayColorSpaceSupport {
                    rgb: true,
                    ..DisplayColorSpaceSupport::default()
                },
                preferred_quantization: DisplayQuantizationSupport {
                    default: true,
                    ..DisplayQuantizationSupport::default()
                },
                require_audio: true,
                prefer_hdr: true,
                prefer_vrr: false,
                allow_scaling: false,
                allow_interlaced: false,
            })
            .unwrap();

        assert_eq!(result.config.mode.width, 1920);
        assert_eq!(result.config.mode.height, 1080);
        assert_eq!(result.config.pixel_format, DisplayPixelFormat::Xrgb8888);
        assert_eq!(result.config.color_space, DisplayColorSpace::Rgb);
        assert!(result.config.audio_enabled);
        assert!(result.config.hdr_enabled);
    }

    #[test]
    fn display_port_driver_builds_port_handle_and_validates_config() {
        let endpoint = DisplayPortEndpoint::<FakeDisplayPortHardware>::try_new(0).unwrap();
        let port = endpoint.port().unwrap();
        let negotiated = endpoint
            .negotiate(&DisplayNegotiationRequest {
                preferred_modes: &[],
                preferred_pixel_formats: DisplayPixelFormatSupport::default(),
                preferred_color_spaces: DisplayColorSpaceSupport::default(),
                preferred_quantization: DisplayQuantizationSupport::default(),
                require_audio: false,
                prefer_hdr: false,
                prefer_vrr: false,
                allow_scaling: false,
                allow_interlaced: false,
            })
            .unwrap();

        assert!(port.validate_config(&negotiated.config).is_ok());
        assert_eq!(
            port.descriptor().unwrap().connector,
            DisplayConnectorKind::DisplayPort
        );
    }
}
