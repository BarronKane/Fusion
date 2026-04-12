//! Firmware-side ACPI platform matching and public-driver realization.
//!
//! This layer sits between two different kinds of truth:
//!
//! - crawled or externally supplied platform identity and ACPI namespace facts
//! - the canonical public ACPI driver families in `fusion-hal`
//!
//! It does not parse AML, and it does not replace the public ACPI driver contracts. Its job is
//! narrower:
//!
//! - identify which vendor backend should realize a machine,
//! - activate the matching public ACPI driver families over that backend,
//! - expose the realized surfaces upward to later firmware/sys layers through stable contract
//!   traits rather than vendor-specific types.

use crate::contract::firmware::topology::{
    AcpiTopologyContract,
    AcpiTopologySupport,
};
use fusion_hal::contract::drivers::acpi::{
    AcpiBatteryContract,
    AcpiButtonContract,
    AcpiEmbeddedControllerContract,
    AcpiFanContract,
    AcpiLidContract,
    AcpiPowerSourceContract,
    AcpiProcessorContract,
    AcpiThermalContract,
};
use fusion_hal::contract::drivers::driver::{
    DriverActivationContext,
    DriverDiscoveryContext,
    DriverError,
    DriverErrorKind,
    DriverRegistry,
};
use fusion_hal::drivers::acpi::public::battery::{
    AcpiBattery,
    AcpiBatteryBinding,
    AcpiBatteryDriver,
    AcpiBatteryDriverContext,
};
use fusion_hal::drivers::acpi::public::button::{
    AcpiButton,
    AcpiButtonBinding,
    AcpiButtonDriver,
    AcpiButtonDriverContext,
};
use fusion_hal::drivers::acpi::public::embedded_controller::{
    AcpiEmbeddedController,
    AcpiEmbeddedControllerBinding,
    AcpiEmbeddedControllerDriver,
    AcpiEmbeddedControllerDriverContext,
};
use fusion_hal::drivers::acpi::public::fan::{
    AcpiFan,
    AcpiFanBinding,
    AcpiFanDriver,
    AcpiFanDriverContext,
};
use fusion_hal::drivers::acpi::public::lid::{
    AcpiLid,
    AcpiLidBinding,
    AcpiLidDriver,
    AcpiLidDriverContext,
};
use fusion_hal::drivers::acpi::public::power_source::{
    AcpiPowerSource,
    AcpiPowerSourceBinding,
    AcpiPowerSourceDriver,
    AcpiPowerSourceDriverContext,
};
use fusion_hal::drivers::acpi::public::processor::{
    AcpiProcessor,
    AcpiProcessorBinding,
    AcpiProcessorDriver,
    AcpiProcessorDriverContext,
};
use fusion_hal::drivers::acpi::public::thermal::{
    AcpiThermal,
    AcpiThermalBinding,
    AcpiThermalDriver,
    AcpiThermalDriverContext,
};
use fusion_hal::drivers::acpi::vendor::dell::DellLatitudeE6430AcpiHardware;

/// Stable firmware-side fingerprint used to match one ACPI-backed platform realization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AcpiPlatformFingerprint {
    pub system_vendor: &'static str,
    pub product_name: &'static str,
    pub bios_version: Option<&'static str>,
    pub acpi_oem_id: Option<&'static str>,
    pub acpi_oem_table_id: Option<&'static str>,
}

/// Coarse backend family matched for one ACPI-backed platform.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AcpiPlatformBackendKind {
    DellLatitudeE6430,
}

/// Match confidence for one platform/backend selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AcpiPlatformMatchStrength {
    Exact,
}

/// Concrete backend selection made by the firmware ACPI matcher.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AcpiPlatformMatch {
    pub backend: AcpiPlatformBackendKind,
    pub strength: AcpiPlatformMatchStrength,
}

/// Firmware-side ACPI realization error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AcpiRealizationErrorKind {
    Unsupported,
    Invalid,
    Busy,
    ResourceExhausted,
    StateConflict,
    Platform(i32),
}

/// Firmware-side ACPI realization failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AcpiRealizationError {
    kind: AcpiRealizationErrorKind,
}

impl AcpiRealizationError {
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            kind: AcpiRealizationErrorKind::Unsupported,
        }
    }

    #[must_use]
    pub const fn invalid() -> Self {
        Self {
            kind: AcpiRealizationErrorKind::Invalid,
        }
    }

    #[must_use]
    pub const fn busy() -> Self {
        Self {
            kind: AcpiRealizationErrorKind::Busy,
        }
    }

    #[must_use]
    pub const fn resource_exhausted() -> Self {
        Self {
            kind: AcpiRealizationErrorKind::ResourceExhausted,
        }
    }

    #[must_use]
    pub const fn state_conflict() -> Self {
        Self {
            kind: AcpiRealizationErrorKind::StateConflict,
        }
    }

    #[must_use]
    pub const fn platform(code: i32) -> Self {
        Self {
            kind: AcpiRealizationErrorKind::Platform(code),
        }
    }

    #[must_use]
    pub const fn kind(self) -> AcpiRealizationErrorKind {
        self.kind
    }
}

/// Fingerprint captured from the Dell Latitude E6430 proving dump.
#[must_use]
pub const fn dell_latitude_e6430_fingerprint() -> AcpiPlatformFingerprint {
    AcpiPlatformFingerprint {
        system_vendor: "Dell Inc.",
        product_name: "Latitude E6430",
        bios_version: Some("A13"),
        acpi_oem_id: None,
        acpi_oem_table_id: None,
    }
}

/// Returns the backend selection for one supplied platform fingerprint, if any.
#[must_use]
pub fn match_platform_backend(fingerprint: &AcpiPlatformFingerprint) -> Option<AcpiPlatformMatch> {
    if fingerprint.system_vendor == "Dell Inc." && fingerprint.product_name == "Latitude E6430" {
        return Some(AcpiPlatformMatch {
            backend: AcpiPlatformBackendKind::DellLatitudeE6430,
            strength: AcpiPlatformMatchStrength::Exact,
        });
    }

    None
}

/// Firmware-realized public ACPI surfaces for one matched platform.
#[derive(Debug)]
pub struct RealizedAcpiPlatform {
    matched: AcpiPlatformMatch,
    battery: Option<AcpiBattery<DellLatitudeE6430AcpiHardware>>,
    power_source: Option<AcpiPowerSource<DellLatitudeE6430AcpiHardware>>,
    thermal: Option<AcpiThermal<DellLatitudeE6430AcpiHardware>>,
    fan: Option<AcpiFan<DellLatitudeE6430AcpiHardware>>,
    button: Option<AcpiButton<DellLatitudeE6430AcpiHardware>>,
    lid: Option<AcpiLid<DellLatitudeE6430AcpiHardware>>,
    embedded_controller: Option<AcpiEmbeddedController<DellLatitudeE6430AcpiHardware>>,
    processor: Option<AcpiProcessor<DellLatitudeE6430AcpiHardware>>,
}

impl RealizedAcpiPlatform {
    /// Returns the matched backend record for this realized platform.
    #[must_use]
    pub const fn matched(&self) -> AcpiPlatformMatch {
        self.matched
    }

    /// Returns the canonical ACPI battery surface, when one was realized.
    #[must_use]
    pub fn battery(&self) -> Option<&dyn AcpiBatteryContract> {
        self.battery
            .as_ref()
            .map(|surface| surface as &dyn AcpiBatteryContract)
    }

    /// Returns the canonical ACPI power-source surface, when one was realized.
    #[must_use]
    pub fn power_source(&self) -> Option<&dyn AcpiPowerSourceContract> {
        self.power_source
            .as_ref()
            .map(|surface| surface as &dyn AcpiPowerSourceContract)
    }

    /// Returns the canonical ACPI thermal surface, when one was realized.
    #[must_use]
    pub fn thermal(&self) -> Option<&dyn AcpiThermalContract> {
        self.thermal
            .as_ref()
            .map(|surface| surface as &dyn AcpiThermalContract)
    }

    /// Returns the canonical ACPI fan surface, when one was realized.
    #[must_use]
    pub fn fan(&self) -> Option<&dyn AcpiFanContract> {
        self.fan
            .as_ref()
            .map(|surface| surface as &dyn AcpiFanContract)
    }

    /// Returns the canonical ACPI button/switch surface, when one was realized.
    #[must_use]
    pub fn button(&self) -> Option<&dyn AcpiButtonContract> {
        self.button
            .as_ref()
            .map(|surface| surface as &dyn AcpiButtonContract)
    }

    /// Returns the canonical ACPI lid surface, when one was realized.
    #[must_use]
    pub fn lid(&self) -> Option<&dyn AcpiLidContract> {
        self.lid
            .as_ref()
            .map(|surface| surface as &dyn AcpiLidContract)
    }

    /// Returns the canonical ACPI embedded-controller surface, when one was realized.
    #[must_use]
    pub fn embedded_controller(&self) -> Option<&dyn AcpiEmbeddedControllerContract> {
        self.embedded_controller
            .as_ref()
            .map(|surface| surface as &dyn AcpiEmbeddedControllerContract)
    }

    /// Returns the canonical ACPI processor surface, when one was realized.
    #[must_use]
    pub fn processor(&self) -> Option<&dyn AcpiProcessorContract> {
        self.processor
            .as_ref()
            .map(|surface| surface as &dyn AcpiProcessorContract)
    }
}

impl AcpiTopologyContract for RealizedAcpiPlatform {
    fn acpi_topology_support(&self) -> AcpiTopologySupport {
        AcpiTopologySupport::StaticTables
    }
}

/// Matches and realizes one platform from the supplied firmware/platform fingerprint.
///
/// # Errors
///
/// Returns one honest error when no supported backend matches or when public ACPI driver
/// activation fails.
pub fn realize_platform(
    fingerprint: &AcpiPlatformFingerprint,
) -> Result<RealizedAcpiPlatform, AcpiRealizationError> {
    let matched =
        match_platform_backend(fingerprint).ok_or_else(AcpiRealizationError::unsupported)?;

    match matched.backend {
        AcpiPlatformBackendKind::DellLatitudeE6430 => realize_dell_latitude_e6430(matched),
    }
}

/// Realizes the Dell Latitude E6430 proving platform directly from the captured fingerprint.
///
/// # Errors
///
/// Returns one honest error when public ACPI driver activation fails.
pub fn realize_dell_latitude_e6430_platform() -> Result<RealizedAcpiPlatform, AcpiRealizationError>
{
    realize_platform(&dell_latitude_e6430_fingerprint())
}

fn realize_dell_latitude_e6430(
    matched: AcpiPlatformMatch,
) -> Result<RealizedAcpiPlatform, AcpiRealizationError> {
    let mut registry = DriverRegistry::<8>::new();

    let battery = activate_battery::<DellLatitudeE6430AcpiHardware>(&mut registry)?;
    let power_source = activate_power_source::<DellLatitudeE6430AcpiHardware>(&mut registry)?;
    let thermal = activate_thermal::<DellLatitudeE6430AcpiHardware>(&mut registry)?;
    let fan = activate_fan::<DellLatitudeE6430AcpiHardware>(&mut registry)?;
    let button = activate_button::<DellLatitudeE6430AcpiHardware>(&mut registry)?;
    let lid = activate_lid::<DellLatitudeE6430AcpiHardware>(&mut registry)?;
    let embedded_controller =
        activate_embedded_controller::<DellLatitudeE6430AcpiHardware>(&mut registry)?;
    let processor = activate_processor::<DellLatitudeE6430AcpiHardware>(&mut registry)?;

    Ok(RealizedAcpiPlatform {
        matched,
        battery,
        power_source,
        thermal,
        fan,
        button,
        lid,
        embedded_controller,
        processor,
    })
}

fn activate_battery<H>(
    registry: &mut DriverRegistry<8>,
) -> Result<Option<AcpiBattery<H>>, AcpiRealizationError>
where
    H: fusion_hal::drivers::acpi::public::interface::contract::AcpiBatteryHardware + 'static,
{
    let registered = registry
        .register::<AcpiBatteryDriver<H>>()
        .map_err(map_driver_error)?;
    let mut context = AcpiBatteryDriverContext::<H>::new();
    let mut bindings = [AcpiBatteryBinding {
        provider: 0,
        provider_id: "",
    }];
    let count = {
        let mut discovery = DriverDiscoveryContext::new(&mut context);
        registered
            .enumerate_bindings(&mut discovery, &mut bindings)
            .map_err(map_driver_error)?
    };
    if count == 0 {
        return Ok(None);
    }
    let mut activation = DriverActivationContext::new(&mut context);
    registered
        .activate(&mut activation, bindings[0])
        .map(|driver| Some(driver.into_instance()))
        .map_err(map_driver_error)
}

fn activate_power_source<H>(
    registry: &mut DriverRegistry<8>,
) -> Result<Option<AcpiPowerSource<H>>, AcpiRealizationError>
where
    H: fusion_hal::drivers::acpi::public::interface::contract::AcpiPowerSourceHardware + 'static,
{
    let registered = registry
        .register::<AcpiPowerSourceDriver<H>>()
        .map_err(map_driver_error)?;
    let mut context = AcpiPowerSourceDriverContext::<H>::new();
    let mut bindings = [AcpiPowerSourceBinding {
        provider: 0,
        provider_id: "",
    }];
    let count = {
        let mut discovery = DriverDiscoveryContext::new(&mut context);
        registered
            .enumerate_bindings(&mut discovery, &mut bindings)
            .map_err(map_driver_error)?
    };
    if count == 0 {
        return Ok(None);
    }
    let mut activation = DriverActivationContext::new(&mut context);
    registered
        .activate(&mut activation, bindings[0])
        .map(|driver| Some(driver.into_instance()))
        .map_err(map_driver_error)
}

fn activate_thermal<H>(
    registry: &mut DriverRegistry<8>,
) -> Result<Option<AcpiThermal<H>>, AcpiRealizationError>
where
    H: fusion_hal::drivers::acpi::public::interface::contract::AcpiThermalHardware + 'static,
{
    let registered = registry
        .register::<AcpiThermalDriver<H>>()
        .map_err(map_driver_error)?;
    let mut context = AcpiThermalDriverContext::<H>::new();
    let mut bindings = [AcpiThermalBinding {
        provider: 0,
        provider_id: "",
    }];
    let count = {
        let mut discovery = DriverDiscoveryContext::new(&mut context);
        registered
            .enumerate_bindings(&mut discovery, &mut bindings)
            .map_err(map_driver_error)?
    };
    if count == 0 {
        return Ok(None);
    }
    let mut activation = DriverActivationContext::new(&mut context);
    registered
        .activate(&mut activation, bindings[0])
        .map(|driver| Some(driver.into_instance()))
        .map_err(map_driver_error)
}

fn activate_fan<H>(
    registry: &mut DriverRegistry<8>,
) -> Result<Option<AcpiFan<H>>, AcpiRealizationError>
where
    H: fusion_hal::drivers::acpi::public::interface::contract::AcpiFanHardware + 'static,
{
    let registered = registry
        .register::<AcpiFanDriver<H>>()
        .map_err(map_driver_error)?;
    let mut context = AcpiFanDriverContext::<H>::new();
    let mut bindings = [AcpiFanBinding {
        provider: 0,
        provider_id: "",
    }];
    let count = {
        let mut discovery = DriverDiscoveryContext::new(&mut context);
        registered
            .enumerate_bindings(&mut discovery, &mut bindings)
            .map_err(map_driver_error)?
    };
    if count == 0 {
        return Ok(None);
    }
    let mut activation = DriverActivationContext::new(&mut context);
    registered
        .activate(&mut activation, bindings[0])
        .map(|driver| Some(driver.into_instance()))
        .map_err(map_driver_error)
}

fn activate_button<H>(
    registry: &mut DriverRegistry<8>,
) -> Result<Option<AcpiButton<H>>, AcpiRealizationError>
where
    H: fusion_hal::drivers::acpi::public::interface::contract::AcpiButtonHardware + 'static,
{
    let registered = registry
        .register::<AcpiButtonDriver<H>>()
        .map_err(map_driver_error)?;
    let mut context = AcpiButtonDriverContext::<H>::new();
    let mut bindings = [AcpiButtonBinding {
        provider: 0,
        provider_id: "",
    }];
    let count = {
        let mut discovery = DriverDiscoveryContext::new(&mut context);
        registered
            .enumerate_bindings(&mut discovery, &mut bindings)
            .map_err(map_driver_error)?
    };
    if count == 0 {
        return Ok(None);
    }
    let mut activation = DriverActivationContext::new(&mut context);
    registered
        .activate(&mut activation, bindings[0])
        .map(|driver| Some(driver.into_instance()))
        .map_err(map_driver_error)
}

fn activate_lid<H>(
    registry: &mut DriverRegistry<8>,
) -> Result<Option<AcpiLid<H>>, AcpiRealizationError>
where
    H: fusion_hal::drivers::acpi::public::interface::contract::AcpiLidHardware + 'static,
{
    let registered = registry
        .register::<AcpiLidDriver<H>>()
        .map_err(map_driver_error)?;
    let mut context = AcpiLidDriverContext::<H>::new();
    let mut bindings = [AcpiLidBinding {
        provider: 0,
        provider_id: "",
    }];
    let count = {
        let mut discovery = DriverDiscoveryContext::new(&mut context);
        registered
            .enumerate_bindings(&mut discovery, &mut bindings)
            .map_err(map_driver_error)?
    };
    if count == 0 {
        return Ok(None);
    }
    let mut activation = DriverActivationContext::new(&mut context);
    registered
        .activate(&mut activation, bindings[0])
        .map(|driver| Some(driver.into_instance()))
        .map_err(map_driver_error)
}

fn activate_embedded_controller<H>(
    registry: &mut DriverRegistry<8>,
) -> Result<Option<AcpiEmbeddedController<H>>, AcpiRealizationError>
where
    H: fusion_hal::drivers::acpi::public::interface::contract::AcpiEmbeddedControllerHardware
        + 'static,
{
    let registered = registry
        .register::<AcpiEmbeddedControllerDriver<H>>()
        .map_err(map_driver_error)?;
    let mut context = AcpiEmbeddedControllerDriverContext::<H>::new();
    let mut bindings = [AcpiEmbeddedControllerBinding {
        provider: 0,
        provider_id: "",
    }];
    let count = {
        let mut discovery = DriverDiscoveryContext::new(&mut context);
        registered
            .enumerate_bindings(&mut discovery, &mut bindings)
            .map_err(map_driver_error)?
    };
    if count == 0 {
        return Ok(None);
    }
    let mut activation = DriverActivationContext::new(&mut context);
    registered
        .activate(&mut activation, bindings[0])
        .map(|driver| Some(driver.into_instance()))
        .map_err(map_driver_error)
}

fn activate_processor<H>(
    registry: &mut DriverRegistry<8>,
) -> Result<Option<AcpiProcessor<H>>, AcpiRealizationError>
where
    H: fusion_hal::drivers::acpi::public::interface::contract::AcpiProcessorHardware + 'static,
{
    let registered = registry
        .register::<AcpiProcessorDriver<H>>()
        .map_err(map_driver_error)?;
    let mut context = AcpiProcessorDriverContext::<H>::new();
    let mut bindings = [AcpiProcessorBinding {
        provider: 0,
        provider_id: "",
    }];
    let count = {
        let mut discovery = DriverDiscoveryContext::new(&mut context);
        registered
            .enumerate_bindings(&mut discovery, &mut bindings)
            .map_err(map_driver_error)?
    };
    if count == 0 {
        return Ok(None);
    }
    let mut activation = DriverActivationContext::new(&mut context);
    registered
        .activate(&mut activation, bindings[0])
        .map(|driver| Some(driver.into_instance()))
        .map_err(map_driver_error)
}

fn map_driver_error(error: DriverError) -> AcpiRealizationError {
    match error.kind() {
        DriverErrorKind::Unsupported => AcpiRealizationError::unsupported(),
        DriverErrorKind::Invalid => AcpiRealizationError::invalid(),
        DriverErrorKind::Busy => AcpiRealizationError::busy(),
        DriverErrorKind::ResourceExhausted => AcpiRealizationError::resource_exhausted(),
        DriverErrorKind::StateConflict => AcpiRealizationError::state_conflict(),
        DriverErrorKind::MissingContext
        | DriverErrorKind::WrongContextType
        | DriverErrorKind::AlreadyRegistered => AcpiRealizationError::state_conflict(),
        DriverErrorKind::Platform(code) => AcpiRealizationError::platform(code),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dell_platform_matches_exactly() {
        let matched = match_platform_backend(&dell_latitude_e6430_fingerprint())
            .expect("dell latitude e6430 should match");
        assert_eq!(matched.backend, AcpiPlatformBackendKind::DellLatitudeE6430);
        assert_eq!(matched.strength, AcpiPlatformMatchStrength::Exact);
    }

    #[test]
    fn dell_platform_realization_activates_expected_components() {
        let realized =
            realize_dell_latitude_e6430_platform().expect("dell realization should work");

        assert_eq!(
            realized.acpi_topology_support(),
            AcpiTopologySupport::StaticTables
        );
        assert!(realized.battery().is_some());
        assert!(realized.power_source().is_some());
        assert!(realized.thermal().is_some());
        assert!(realized.button().is_some());
        assert!(realized.lid().is_some());
        assert!(realized.embedded_controller().is_some());
        assert!(realized.fan().is_none());
        assert!(realized.processor().is_none());
    }
}
