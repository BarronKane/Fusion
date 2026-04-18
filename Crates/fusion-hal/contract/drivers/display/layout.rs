//! Public machine-display composition contract vocabulary.

use super::control::DisplayControlContract;
use super::error::DisplayResult;
use super::types::{
    DisplayLayoutConfig,
    DisplayLayoutPresentReport,
    DisplayLayoutPresentRequest,
    DisplayLayoutState,
    DisplayLayoutValidationError,
    DisplayOutputDescriptor,
    DisplayOutputId,
    DisplaySurfacePlacement,
};
use super::DisplaySurfaceId;

/// Canonical machine-display composition surface.
pub trait DisplayLayoutContract {
    /// Concrete borrowed control handle/view returned for one display output.
    ///
    /// This is not intended to imply a detached copy of control state. The returned value should
    /// behave like a lightweight view into layout-owned display state for the lifetime `'a`.
    type Control<'a>: DisplayControlContract
    where
        Self: 'a;

    /// Enumerates currently surfaced display outputs.
    ///
    /// # Errors
    ///
    /// Returns one honest error when output enumeration cannot complete.
    fn enumerate_outputs(&self, out: &mut [DisplayOutputId]) -> DisplayResult<usize>;

    /// Returns the static descriptor for one surfaced output when visible.
    ///
    /// # Errors
    ///
    /// Returns one honest error when descriptor retrieval fails.
    fn output_descriptor(
        &self,
        id: DisplayOutputId,
    ) -> DisplayResult<Option<DisplayOutputDescriptor>>;

    /// Returns current global layout/composition state.
    fn layout_state(&self) -> DisplayResult<DisplayLayoutState>;

    /// Validates one requested layout against machine display truth.
    ///
    /// Validation errors are surfaced as `DisplayLayoutValidationError` directly rather than
    /// being wrapped in `DisplayError`, because this path is preflight contract checking rather
    /// than an operational driver failure.
    fn validate_layout(
        &self,
        layout: &DisplayLayoutConfig<'_>,
    ) -> Result<(), DisplayLayoutValidationError>;

    /// Applies one requested layout.
    fn apply_layout(&mut self, layout: &DisplayLayoutConfig<'_>) -> DisplayResult<()>;

    /// Returns the current primary output when one exists.
    fn primary_output(&self) -> DisplayResult<Option<DisplayOutputId>>;

    /// Sets or clears the current primary output.
    fn set_primary_output(&mut self, output: Option<DisplayOutputId>) -> DisplayResult<()>;

    /// Returns the per-display control surface for one visible output.
    ///
    /// The returned control surface is a borrowed handle/view and owns access to its associated
    /// port surface.
    fn control(&self, id: DisplayOutputId) -> DisplayResult<Option<Self::Control<'_>>>;

    /// Returns mutable per-display control access for one visible output.
    ///
    /// The returned control surface is still a borrowed handle/view, not a detached control copy.
    fn control_mut(&mut self, id: DisplayOutputId) -> DisplayResult<Option<Self::Control<'_>>>;

    /// Places or re-places one surfaced composition surface on one output.
    fn place_surface(
        &mut self,
        surface: DisplaySurfaceId,
        placement: &DisplaySurfacePlacement,
    ) -> DisplayResult<()>;

    /// Presents one machine-wide layout update.
    fn present_layout(
        &mut self,
        request: &DisplayLayoutPresentRequest<'_>,
    ) -> DisplayResult<DisplayLayoutPresentReport>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::drivers::display::core::DisplayConnectorKind;

    #[test]
    fn display_output_descriptor_keeps_connector_identity() {
        let descriptor = DisplayOutputDescriptor {
            id: DisplayOutputId(1),
            name: "hdmi-0",
            connector: DisplayConnectorKind::Hdmi,
            hotplug_supported: true,
        };
        assert_eq!(descriptor.id, DisplayOutputId(1));
        assert_eq!(descriptor.connector, DisplayConnectorKind::Hdmi);
    }
}
