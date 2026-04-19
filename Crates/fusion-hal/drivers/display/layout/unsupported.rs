//! Unsupported machine-display layout backend placeholders.

use crate::contract::drivers::display::{
    DisplayActiveConfig,
    DisplayConfigError,
    DisplayControlContract,
    DisplayControlState,
    DisplayDescriptorSet,
    DisplayFeature,
    DisplayFeatureCapabilities,
    DisplayFeatureValue,
    DisplayFrameView,
    DisplayHotplugEvent,
    DisplayIdentity,
    DisplayLayoutConfig,
    DisplayLayoutPresentReport,
    DisplayLayoutPresentRequest,
    DisplayLayoutState,
    DisplayLayoutValidationError,
    DisplayNegotiationRequest,
    DisplayNegotiationResult,
    DisplayOutputDescriptor,
    DisplayOutputId,
    DisplayPortCapabilities,
    DisplayPortContract,
    DisplayPortDescriptor,
    DisplayPortState,
    DisplayPowerState,
    DisplayPresentReport,
    DisplayPresentRequest,
    DisplayResult,
    DisplaySinkCapabilities,
    DisplaySurfaceBinding,
    DisplaySurfaceId,
    DisplaySurfacePlacement,
    DisplayTiming,
    DisplayUploadReport,
};

use super::DisplayLayoutHardware;

/// Unsupported layout backend placeholder used as the default type parameter for the canonical
/// display-layout driver family.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedDisplayLayoutHardware;

/// Unsupported control handle placeholder surfaced when no real display connector backend exists.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedDisplayControl;

/// Unsupported port handle placeholder surfaced when no real display connector backend exists.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedDisplayPort;

impl DisplayControlContract for UnsupportedDisplayControl {
    type Port<'a>
        = UnsupportedDisplayPort
    where
        Self: 'a;

    fn state(&self) -> DisplayResult<DisplayControlState> {
        Err(crate::contract::drivers::display::DisplayError::unsupported())
    }

    fn refresh(&mut self) -> DisplayResult<()> {
        Err(crate::contract::drivers::display::DisplayError::unsupported())
    }

    fn identify(&self) -> DisplayResult<DisplayIdentity> {
        Err(crate::contract::drivers::display::DisplayError::unsupported())
    }

    fn sink_capabilities(&self) -> DisplayResult<DisplaySinkCapabilities<'_>> {
        Err(crate::contract::drivers::display::DisplayError::unsupported())
    }

    fn raw_descriptors(&self) -> DisplayResult<DisplayDescriptorSet<'_>> {
        Err(crate::contract::drivers::display::DisplayError::unsupported())
    }

    fn negotiate(
        &self,
        _request: &DisplayNegotiationRequest<'_>,
    ) -> DisplayResult<DisplayNegotiationResult> {
        Err(crate::contract::drivers::display::DisplayError::unsupported())
    }

    fn feature_capabilities(&self) -> DisplayResult<DisplayFeatureCapabilities> {
        Err(crate::contract::drivers::display::DisplayError::unsupported())
    }

    fn get_feature(&self, _feature: DisplayFeature) -> DisplayResult<DisplayFeatureValue> {
        Err(crate::contract::drivers::display::DisplayError::unsupported())
    }

    fn set_feature(
        &mut self,
        _feature: DisplayFeature,
        _value: DisplayFeatureValue,
    ) -> DisplayResult<()> {
        Err(crate::contract::drivers::display::DisplayError::unsupported())
    }

    fn power_state(&self) -> DisplayResult<DisplayPowerState> {
        Err(crate::contract::drivers::display::DisplayError::unsupported())
    }

    fn set_power_state(&mut self, _state: DisplayPowerState) -> DisplayResult<()> {
        Err(crate::contract::drivers::display::DisplayError::unsupported())
    }

    fn port(&self) -> DisplayResult<Self::Port<'_>> {
        Ok(UnsupportedDisplayPort)
    }

    fn port_mut(&mut self) -> DisplayResult<Self::Port<'_>> {
        Ok(UnsupportedDisplayPort)
    }
}

impl DisplayPortContract for UnsupportedDisplayPort {
    fn descriptor(&self) -> DisplayResult<DisplayPortDescriptor> {
        Err(crate::contract::drivers::display::DisplayError::unsupported())
    }

    fn state(&self) -> DisplayResult<DisplayPortState> {
        Err(crate::contract::drivers::display::DisplayError::unsupported())
    }

    fn capabilities(&self) -> DisplayResult<DisplayPortCapabilities> {
        Err(crate::contract::drivers::display::DisplayError::unsupported())
    }

    fn validate_config(&self, _config: &DisplayActiveConfig) -> Result<(), DisplayConfigError> {
        Err(DisplayConfigError::NotReady)
    }

    fn active_config(&self) -> DisplayResult<Option<DisplayActiveConfig>> {
        Err(crate::contract::drivers::display::DisplayError::unsupported())
    }

    fn set_config(&mut self, _config: &DisplayActiveConfig) -> DisplayResult<()> {
        Err(crate::contract::drivers::display::DisplayError::unsupported())
    }

    fn enable(&mut self) -> DisplayResult<()> {
        Err(crate::contract::drivers::display::DisplayError::unsupported())
    }

    fn disable(&mut self) -> DisplayResult<()> {
        Err(crate::contract::drivers::display::DisplayError::unsupported())
    }

    fn blank(&mut self, _blanked: bool) -> DisplayResult<()> {
        Err(crate::contract::drivers::display::DisplayError::unsupported())
    }

    fn attach_surface(&mut self, _surface: DisplaySurfaceBinding) -> DisplayResult<()> {
        Err(crate::contract::drivers::display::DisplayError::unsupported())
    }

    fn detach_surface(&mut self) -> DisplayResult<()> {
        Err(crate::contract::drivers::display::DisplayError::unsupported())
    }

    fn upload_frame(
        &mut self,
        _frame: &DisplayFrameView<'_>,
        _region: Option<crate::contract::drivers::display::DisplayRegion>,
    ) -> DisplayResult<DisplayUploadReport> {
        Err(crate::contract::drivers::display::DisplayError::unsupported())
    }

    fn present(&mut self, _request: &DisplayPresentRequest) -> DisplayResult<DisplayPresentReport> {
        Err(crate::contract::drivers::display::DisplayError::unsupported())
    }

    fn flush(&mut self) -> DisplayResult<()> {
        Err(crate::contract::drivers::display::DisplayError::unsupported())
    }

    fn wait_vblank(&mut self, _timeout_ms: u32) -> DisplayResult<()> {
        Err(crate::contract::drivers::display::DisplayError::unsupported())
    }

    fn wait_hotplug_event(
        &mut self,
        _timeout_ms: u32,
    ) -> DisplayResult<Option<DisplayHotplugEvent>> {
        Err(crate::contract::drivers::display::DisplayError::unsupported())
    }

    fn timing(&self) -> DisplayResult<Option<DisplayTiming>> {
        Err(crate::contract::drivers::display::DisplayError::unsupported())
    }
}

impl DisplayLayoutHardware for UnsupportedDisplayLayoutHardware {
    type Control<'a>
        = UnsupportedDisplayControl
    where
        Self: 'a;

    fn layout_count() -> u8 {
        0
    }

    fn layout_id(_layout: u8) -> Option<&'static str> {
        None
    }

    fn enumerate_outputs(_layout: u8, _out: &mut [DisplayOutputId]) -> DisplayResult<usize> {
        Ok(0)
    }

    fn output_descriptor(
        _layout: u8,
        _id: DisplayOutputId,
    ) -> DisplayResult<Option<DisplayOutputDescriptor>> {
        Ok(None)
    }

    fn layout_state(_layout: u8) -> DisplayResult<DisplayLayoutState> {
        Ok(DisplayLayoutState::default())
    }

    fn validate_layout(
        _layout: u8,
        _config: &DisplayLayoutConfig<'_>,
    ) -> Result<(), DisplayLayoutValidationError> {
        Err(DisplayLayoutValidationError::NotReady)
    }

    fn apply_layout(_layout: u8, _config: &DisplayLayoutConfig<'_>) -> DisplayResult<()> {
        Err(crate::contract::drivers::display::DisplayError::unsupported())
    }

    fn primary_output(_layout: u8) -> DisplayResult<Option<DisplayOutputId>> {
        Ok(None)
    }

    fn set_primary_output(_layout: u8, _output: Option<DisplayOutputId>) -> DisplayResult<()> {
        Err(crate::contract::drivers::display::DisplayError::unsupported())
    }

    fn control<'a>(_layout: u8, _id: DisplayOutputId) -> DisplayResult<Option<Self::Control<'a>>> {
        Ok(None)
    }

    fn control_mut<'a>(
        _layout: u8,
        _id: DisplayOutputId,
    ) -> DisplayResult<Option<Self::Control<'a>>> {
        Ok(None)
    }

    fn place_surface(
        _layout: u8,
        _surface: DisplaySurfaceId,
        _placement: &DisplaySurfacePlacement,
    ) -> DisplayResult<()> {
        Err(crate::contract::drivers::display::DisplayError::unsupported())
    }

    fn present_layout(
        _layout: u8,
        _request: &DisplayLayoutPresentRequest<'_>,
    ) -> DisplayResult<DisplayLayoutPresentReport> {
        Err(crate::contract::drivers::display::DisplayError::unsupported())
    }
}
