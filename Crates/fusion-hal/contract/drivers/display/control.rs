//! Public sink-management and capability-discovery display contract.

use super::port::DisplayPortContract;
use super::core::{
    DisplayFeature,
    DisplayFeatureValue,
    DisplayIdentity,
    DisplayPowerState,
};
use super::error::DisplayResult;
use super::types::{
    DisplayControlState,
    DisplayDescriptorSet,
    DisplayFeatureCapabilities,
    DisplayNegotiationRequest,
    DisplayNegotiationResult,
    DisplaySinkCapabilities,
};

/// Public management and capability surface for one display sink.
pub trait DisplayControlContract {
    /// Concrete opened port surface owned by this control surface.
    type Port<'a>: DisplayPortContract
    where
        Self: 'a;

    /// Returns current control-plane state for the sink and its cached descriptors.
    fn state(&self) -> DisplayResult<DisplayControlState>;

    /// Refreshes sink discovery/cached descriptors from the implementation's internal control path.
    fn refresh(&mut self) -> DisplayResult<()>;

    /// Returns stable sink identity truth.
    fn identify(&self) -> DisplayResult<DisplayIdentity>;

    /// Returns the current sink capability summary.
    fn sink_capabilities(&self) -> DisplayResult<DisplaySinkCapabilities<'_>>;

    /// Returns the raw descriptors currently known for this sink.
    fn raw_descriptors(&self) -> DisplayResult<DisplayDescriptorSet<'_>>;

    /// Negotiates one requested output policy against the sink and current implementation truth.
    fn negotiate(
        &self,
        request: &DisplayNegotiationRequest<'_>,
    ) -> DisplayResult<DisplayNegotiationResult>;

    /// Returns which monitor-management features are surfaced honestly.
    fn feature_capabilities(&self) -> DisplayResult<DisplayFeatureCapabilities>;

    /// Reads the current value for one surfaced monitor-management feature.
    fn get_feature(&self, feature: DisplayFeature) -> DisplayResult<DisplayFeatureValue>;

    /// Sets one surfaced monitor-management feature.
    fn set_feature(
        &mut self,
        feature: DisplayFeature,
        value: DisplayFeatureValue,
    ) -> DisplayResult<()>;

    /// Returns current sink power state.
    fn power_state(&self) -> DisplayResult<DisplayPowerState>;

    /// Requests one sink power state.
    fn set_power_state(&mut self, state: DisplayPowerState) -> DisplayResult<()>;

    /// Returns the associated port surface for this display.
    fn port(&self) -> DisplayResult<Self::Port<'_>>;

    /// Returns mutable access to the associated port surface for this display.
    fn port_mut(&mut self) -> DisplayResult<Self::Port<'_>>;
}
