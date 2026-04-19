//! HDMI display endpoint driver family.

use core::marker::PhantomData;

use super::edid::{
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
use super::support::{
    map_config_error,
    map_display_error,
};
use crate::contract::drivers::display::{
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
    DisplayRegion,
    DisplayResult,
    DisplaySinkCapabilities,
    DisplaySurfaceBinding,
    DisplayTiming,
    DisplayUploadReport,
};
use crate::contract::drivers::driver::{
    ActiveDriver,
    DriverActivation,
    DriverActivationContext,
    DriverBindingSource,
    DriverClass,
    DriverContract,
    DriverContractKey,
    DriverDiscoveryContext,
    DriverError,
    DriverIdentity,
    DriverMetadata,
    DriverRegistration,
    RegisteredDriver,
};

#[cfg(test)]
use super::edid::EDID_BLOCK_BYTES;
#[cfg(test)]
use crate::contract::drivers::display::{
    DisplayColorSpace,
    DisplayColorSpaceSupport,
    DisplayPixelFormat,
    DisplayPixelFormatSupport,
    DisplayQuantizationSupport,
    DisplayRawDescriptorKind,
};
#[cfg(test)]
use super::edid::lookup_cea_mode;

const HDMI_DRIVER_CONTRACTS: [DriverContractKey; 2] = [
    DriverContractKey("display.control"),
    DriverContractKey("display.port"),
];
const HDMI_DRIVER_BINDING_SOURCES: [DriverBindingSource; 6] = [
    DriverBindingSource::StaticSoc,
    DriverBindingSource::BoardManifest,
    DriverBindingSource::Acpi,
    DriverBindingSource::Devicetree,
    DriverBindingSource::Pci,
    DriverBindingSource::Manual,
];
const HDMI_DRIVER_METADATA: DriverMetadata = DriverMetadata {
    key: "display.port.hdmi",
    class: DriverClass::Display,
    identity: DriverIdentity {
        vendor: "Fusion",
        family: Some("Display"),
        package: None,
        product: "HDMI driver",
        advertised_interface: "HDMI endpoint",
    },
    contracts: &HDMI_DRIVER_CONTRACTS,
    binding_sources: &HDMI_DRIVER_BINDING_SOURCES,
    description: "HDMI display endpoint driver layered over one selected HDMI hardware substrate",
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HdmiBinding {
    pub provider: u8,
    pub output_name: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct HdmiRuntimeState {
    pub enabled: bool,
    pub blanked: bool,
    pub active_config: Option<DisplayActiveConfig>,
}

/// Hardware-facing HDMI seam consumed by the HDMI driver family.
///
/// The public HDMI driver owns EDID parsing, sink identity, capability derivation, negotiation,
/// and config validation. The backend only needs to surface raw descriptors, connector truth, and
/// the actual output programming/presentation operations.
pub trait HdmiHardware {
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
    fn runtime_state(provider: u8) -> DisplayResult<HdmiRuntimeState>;
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
pub struct UnsupportedHdmiHardware;

impl HdmiHardware for UnsupportedHdmiHardware {
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

    fn runtime_state(_provider: u8) -> DisplayResult<HdmiRuntimeState> {
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
pub struct HdmiDriver<H: HdmiHardware = UnsupportedHdmiHardware> {
    marker: PhantomData<fn() -> H>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HdmiDriverContext<H: HdmiHardware = UnsupportedHdmiHardware> {
    marker: PhantomData<fn() -> H>,
}

impl<H> HdmiDriverContext<H>
where
    H: HdmiHardware,
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
    &HDMI_DRIVER_METADATA
}

type HdmiSinkCache = ParsedEdidSink;

#[derive(Debug, Clone)]
pub struct Hdmi<H: HdmiHardware = UnsupportedHdmiHardware> {
    provider: u8,
    sink: HdmiSinkCache,
    _hardware: PhantomData<H>,
}

impl<H> Hdmi<H>
where
    H: HdmiHardware,
{
    fn try_new(provider: u8) -> DisplayResult<Self> {
        let sink = parse_edid_sink(DisplayConnectorKind::Hdmi, H::raw_descriptors(provider)?);
        Ok(Self {
            provider,
            sink,
            _hardware: PhantomData,
        })
    }

    fn refresh_sink(&mut self) -> DisplayResult<()> {
        self.sink = parse_edid_sink(
            DisplayConnectorKind::Hdmi,
            H::raw_descriptors(self.provider)?,
        );
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
pub struct HdmiPort<'a, H: HdmiHardware = UnsupportedHdmiHardware> {
    hdmi: &'a Hdmi<H>,
}

impl<'a, H> HdmiPort<'a, H>
where
    H: HdmiHardware,
{
    fn new(hdmi: &'a Hdmi<H>) -> Self {
        Self { hdmi }
    }

    fn provider(&self) -> u8 {
        self.hdmi.provider
    }
}

impl<H> DisplayControlContract for Hdmi<H>
where
    H: HdmiHardware,
{
    type Port<'a>
        = HdmiPort<'a, H>
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
        Ok(HdmiPort::new(self))
    }

    fn port_mut(&mut self) -> DisplayResult<Self::Port<'_>> {
        Ok(HdmiPort::new(self))
    }
}

impl<'a, H> DisplayPortContract for HdmiPort<'a, H>
where
    H: HdmiHardware,
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
        self.hdmi.validate_active_config(config)
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

fn enumerate_hdmi_bindings<H>(
    _registered: &RegisteredDriver<HdmiDriver<H>>,
    context: &mut DriverDiscoveryContext<'_>,
    out: &mut [HdmiBinding],
) -> Result<usize, DriverError>
where
    H: HdmiHardware + 'static,
{
    let _ = context.downcast_mut::<HdmiDriverContext<H>>()?;
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
        if descriptor.connector != DisplayConnectorKind::Hdmi {
            continue;
        }
        out[written] = HdmiBinding {
            provider,
            output_name: descriptor.name,
        };
        written += 1;
    }

    Ok(written)
}

fn activate_hdmi_binding<H>(
    _registered: &RegisteredDriver<HdmiDriver<H>>,
    context: &mut DriverActivationContext<'_>,
    binding: HdmiBinding,
) -> Result<ActiveDriver<HdmiDriver<H>>, DriverError>
where
    H: HdmiHardware + 'static,
{
    let _ = context.downcast_mut::<HdmiDriverContext<H>>()?;
    let Some(descriptor) = H::output_descriptor(binding.provider) else {
        return Err(DriverError::invalid());
    };
    if descriptor.name != binding.output_name {
        return Err(DriverError::invalid());
    }

    let instance = Hdmi::<H>::try_new(binding.provider).map_err(map_display_error)?;
    Ok(ActiveDriver::new(binding, instance))
}

impl<H> DriverContract for HdmiDriver<H>
where
    H: HdmiHardware + 'static,
{
    type Binding = HdmiBinding;
    type Instance = Hdmi<H>;

    fn registration() -> DriverRegistration<Self> {
        DriverRegistration::new(
            driver_metadata,
            DriverActivation::new(enumerate_hdmi_bindings::<H>, activate_hdmi_binding::<H>),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_OUTPUT_DESCRIPTOR: DisplayOutputDescriptor = DisplayOutputDescriptor {
        id: crate::contract::drivers::display::DisplayOutputId(0),
        name: "hdmi-0",
        connector: DisplayConnectorKind::Hdmi,
        hotplug_supported: true,
    };

    fn build_base_edid() -> [u8; EDID_BLOCK_BYTES] {
        let mut edid = [0u8; EDID_BLOCK_BYTES];
        edid[..8].copy_from_slice(&[0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00]);
        edid[8] = 0x04;
        edid[9] = 0x6d;
        edid[10] = 0x34;
        edid[11] = 0x12;
        edid[12] = 0x78;
        edid[13] = 0x56;
        edid[14] = 0x34;
        edid[15] = 0x12;
        edid[16] = 1;
        edid[17] = 4;
        edid[18] = 0x01;
        edid[19] = 0x03;
        edid[20] = 0x80;
        edid[21] = 0x34;
        edid[22] = 0x20;
        edid[23] = 0x78;
        edid[24] = 0x2a;
        edid[25] = 0xcf;
        edid[26] = 0x74;
        edid[27] = 0xa3;
        edid[28] = 0x57;
        edid[29] = 0x4c;
        edid[30] = 0xb0;
        edid[31] = 0x23;
        edid[32] = 0x09;
        edid[33] = 0x48;
        edid[34] = 0x4c;
        edid[35] = 0x21;
        edid[36] = 0x08;
        edid[37] = 0x00;

        let dtd = &mut edid[54..72];
        dtd.copy_from_slice(&[
            0x02, 0x3a, 0x80, 0x18, 0x71, 0x38, 0x2d, 0x40, 0x58, 0x2c, 0x45, 0x00, 0xa0, 0x5a,
            0x00, 0x00, 0x00, 0x1e,
        ]);

        let name_descriptor = &mut edid[72..90];
        name_descriptor.copy_from_slice(&[
            0x00, 0x00, 0x00, 0xfc, 0x00, b'F', b'u', b's', b'i', b'o', b'n', b' ', b'H', b'D',
            b'M', b'I', 0x0a, 0x20,
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

    fn finalize_edid_checksum(block: &mut [u8; EDID_BLOCK_BYTES]) {
        let checksum = block[..EDID_BLOCK_BYTES - 1]
            .iter()
            .fold(0u8, |sum, byte| sum.wrapping_add(*byte));
        block[EDID_BLOCK_BYTES - 1] = checksum.wrapping_neg();
    }

    struct FakeHdmiHardware;

    impl HdmiHardware for FakeHdmiHardware {
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
            let leaked = Box::leak(edid.into_boxed_slice());
            let descriptors = Box::leak(
                vec![crate::contract::drivers::display::DisplayRawDescriptor {
                    kind: DisplayRawDescriptorKind::Edid,
                    bytes: leaked,
                }]
                .into_boxed_slice(),
            );
            Ok(DisplayDescriptorSet { descriptors })
        }

        fn port_descriptor(_provider: u8) -> DisplayResult<DisplayPortDescriptor> {
            Ok(DisplayPortDescriptor {
                connector: DisplayConnectorKind::Hdmi,
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

        fn runtime_state(_provider: u8) -> DisplayResult<HdmiRuntimeState> {
            Ok(HdmiRuntimeState {
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
                frame_id: crate::contract::drivers::display::DisplayFrameId(1),
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
    fn hdmi_parser_extracts_identity_and_modes() {
        let mut edid = build_base_edid().to_vec();
        edid.extend_from_slice(&build_cta_extension());
        let leaked = Box::leak(edid.into_boxed_slice());
        let descriptors = [crate::contract::drivers::display::DisplayRawDescriptor {
            kind: DisplayRawDescriptorKind::Edid,
            bytes: leaked,
        }];
        let sink = parse_edid_sink(
            DisplayConnectorKind::Hdmi,
            DisplayDescriptorSet {
                descriptors: Box::leak(Box::new(descriptors)),
            },
        );

        assert!(sink.descriptor_valid);
        assert_eq!(sink.identity.product_code, Some(0x1234));
        assert_eq!(
            sink.identity
                .model_name
                .map(|text| text.as_bytes().to_vec()),
            Some(b"Fusion HDMI".to_vec())
        );
        assert!(sink.mode_count >= 2);
        assert!(sink.audio.basic_pcm);
        assert!(sink.color_spaces.ycbcr444);
        assert!(sink.color_spaces.ycbcr422);
        assert!(sink.hdr.hdr_static_metadata);
        assert!(sink.hdr.hdr10);
    }

    #[test]
    fn hdmi_driver_negotiates_requested_mode() {
        let hdmi = Hdmi::<FakeHdmiHardware>::try_new(0).unwrap();
        let result = hdmi
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
    fn hdmi_driver_builds_port_handle_and_validates_config() {
        let hdmi = Hdmi::<FakeHdmiHardware>::try_new(0).unwrap();
        let port = hdmi.port().unwrap();
        let negotiated = hdmi
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
            DisplayConnectorKind::Hdmi
        );
    }

    #[test]
    fn hdmi_cea_table_includes_4k60() {
        let (mode, _) = lookup_cea_mode(97).expect("vic 97");
        assert_eq!(mode.width, 3840);
        assert_eq!(mode.height, 2160);
        assert_eq!(mode.refresh_hz_milli, 60_000);
    }
}
