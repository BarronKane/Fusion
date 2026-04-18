//! Public output/presentation display contract.

use super::core::{
    DisplayRegion,
    DisplayTiming,
};
use super::error::DisplayResult;
use super::types::{
    DisplayActiveConfig,
    DisplayConfigError,
    DisplayFrameView,
    DisplayHotplugEvent,
    DisplayPortCapabilities,
    DisplayPortDescriptor,
    DisplayPortState,
    DisplayPresentReport,
    DisplayPresentRequest,
    DisplaySurfaceBinding,
    DisplayUploadReport,
};

/// Public output and presentation surface for one display connector/path.
pub trait DisplayPortContract {
    /// Returns one static descriptor for this output path.
    fn descriptor(&self) -> DisplayResult<DisplayPortDescriptor>;

    /// Returns current port/output state.
    fn state(&self) -> DisplayResult<DisplayPortState>;

    /// Returns static capability truth for this output path.
    fn capabilities(&self) -> DisplayResult<DisplayPortCapabilities>;

    /// Validates one requested active config against current output truth.
    fn validate_config(&self, config: &DisplayActiveConfig) -> Result<(), DisplayConfigError>;

    /// Returns the currently active output config, when one exists.
    fn active_config(&self) -> DisplayResult<Option<DisplayActiveConfig>>;

    /// Applies one output config.
    fn set_config(&mut self, config: &DisplayActiveConfig) -> DisplayResult<()>;

    /// Enables the output path.
    fn enable(&mut self) -> DisplayResult<()>;

    /// Disables the output path.
    fn disable(&mut self) -> DisplayResult<()>;

    /// Blanks or unblanks the output without necessarily dropping configuration state.
    fn blank(&mut self, blanked: bool) -> DisplayResult<()>;

    /// Attaches one surface for direct scanout or future presentation.
    fn attach_surface(&mut self, surface: DisplaySurfaceBinding) -> DisplayResult<()>;

    /// Detaches any previously attached surface.
    fn detach_surface(&mut self) -> DisplayResult<()>;

    /// Uploads one frame view, optionally scoped to one dirty region.
    fn upload_frame(
        &mut self,
        frame: &DisplayFrameView<'_>,
        region: Option<DisplayRegion>,
    ) -> DisplayResult<DisplayUploadReport>;

    /// Presents one current or selected surface/frame.
    fn present(&mut self, request: &DisplayPresentRequest) -> DisplayResult<DisplayPresentReport>;

    /// Flushes any pending output work.
    fn flush(&mut self) -> DisplayResult<()>;

    /// Waits for one vertical blank interval when supported.
    fn wait_vblank(&mut self, timeout_ms: u32) -> DisplayResult<()>;

    /// Waits for one hotplug/state-change event when supported.
    fn wait_hotplug_event(&mut self, timeout_ms: u32)
    -> DisplayResult<Option<DisplayHotplugEvent>>;

    /// Returns the currently programmed timing when known.
    fn timing(&self) -> DisplayResult<Option<DisplayTiming>>;
}
