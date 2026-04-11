//! PCI hot-plug and slot vocabulary.

/// Slot and hot-plug capability truth for one function/path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PciHotplugProfile {
    pub hotplug_capable: bool,
    pub surprise_hotplug_capable: bool,
    pub power_controller_present: bool,
    pub attention_button_present: bool,
    pub attention_indicator_present: bool,
    pub power_indicator_present: bool,
    pub mrl_sensor_present: bool,
    pub slot_present: Option<bool>,
    pub power_fault: Option<bool>,
    pub latch_open: Option<bool>,
}

/// Hot-plug lane for one PCI function.
pub trait PciHotplugContract {
    /// Returns one truthful hot-plug / slot snapshot when available.
    fn hotplug_profile(&self) -> Option<PciHotplugProfile>;
}
