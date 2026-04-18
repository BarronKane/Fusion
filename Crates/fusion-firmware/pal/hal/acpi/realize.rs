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

use core::mem::MaybeUninit;
use core::slice;

use crate::aml::{
    verify_acpi_backend,
    AmlDefinitionBlock,
    AmlDefinitionBlockSet,
    AmlBackendVerificationIssue,
    AmlBackendVerificationReport,
    AmlError,
    AmlErrorKind,
    AmlLoadedNamespace,
    AmlNamespaceLoadPlan,
    AmlNamespaceLoadRecord,
    AmlRegionAccessHost,
    AmlRuntimeState,
    AmlVm,
    AmlVmLifecycleReport,
    AmlVmState,
};
use crate::contract::firmware::topology::{
    AcpiTopologyContract,
    AcpiTopologySupport,
};
use crate::pal::hal::acpi::{
    AcpiTableView,
    Dsdt,
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
use fusion_hal::drivers::acpi::public::interface::backend::AcpiAmlBackend;
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

/// Firmware-side AML activation report for one realized ACPI platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcpiAmlActivationReport<'a> {
    pub verification: AmlBackendVerificationReport<'a>,
    pub register_regions: AmlVmLifecycleReport,
    pub initialize_devices: AmlVmLifecycleReport,
    pub vm_state: AmlVmState,
}

impl<'a> AcpiAmlActivationReport<'a> {
    #[must_use]
    pub fn is_clean(self) -> bool {
        self.verification.is_clean()
            && self.register_regions.is_clean()
            && self.initialize_devices.is_clean()
            && self.vm_state == AmlVmState::Ready
    }
}

/// Realized ACPI platform plus firmware-side AML activation results.
#[derive(Debug)]
pub struct RealizedAcpiPlatformWithAml<'a> {
    platform: RealizedAcpiPlatform,
    aml: AcpiAmlActivationReport<'a>,
}

impl<'a> RealizedAcpiPlatformWithAml<'a> {
    #[must_use]
    pub const fn platform(&self) -> &RealizedAcpiPlatform {
        &self.platform
    }

    #[must_use]
    pub const fn aml(&self) -> AcpiAmlActivationReport<'a> {
        self.aml
    }

    #[must_use]
    pub fn into_parts(self) -> (RealizedAcpiPlatform, AcpiAmlActivationReport<'a>) {
        (self.platform, self.aml)
    }
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

/// Matches, realizes, verifies, and activates AML for one platform from the supplied fingerprint.
///
/// # Errors
///
/// Returns one honest error when:
/// - no supported backend matches,
/// - public ACPI driver activation fails,
/// - the declared backend AML surface does not match the supplied namespace,
/// - or AML lifecycle activation fails.
pub fn realize_platform_with_aml<'records, 'blocks, 'issues>(
    fingerprint: &AcpiPlatformFingerprint,
    namespace: AmlLoadedNamespace<'records, 'blocks>,
    host: &dyn AmlRegionAccessHost,
    runtime: &AmlRuntimeState<'_>,
    issue_storage: &'issues mut [MaybeUninit<AmlBackendVerificationIssue>],
) -> Result<RealizedAcpiPlatformWithAml<'issues>, AcpiRealizationError> {
    let matched =
        match_platform_backend(fingerprint).ok_or_else(AcpiRealizationError::unsupported)?;

    match matched.backend {
        AcpiPlatformBackendKind::DellLatitudeE6430 => {
            let platform = realize_dell_latitude_e6430(matched)?;
            let aml = activate_backend_aml::<DellLatitudeE6430AcpiHardware>(
                namespace,
                host,
                runtime,
                issue_storage,
            )?;
            Ok(RealizedAcpiPlatformWithAml { platform, aml })
        }
    }
}

/// Loads one AML namespace from validated ACPI definition tables with caller-provided storage.
///
/// `definition_storage` is only used for the secondary definition blocks (`SSDT`/`PSDT`). The
/// `DSDT` block is carried directly by value.
pub fn load_namespace_from_definition_tables<'records, 'tables>(
    dsdt: Dsdt<'tables>,
    secondary_definition_tables: &'tables [AcpiTableView<'tables>],
    definition_storage: &'tables mut [MaybeUninit<AmlDefinitionBlock<'tables>>],
    namespace_storage: &'records mut [MaybeUninit<AmlNamespaceLoadRecord>],
) -> Result<AmlLoadedNamespace<'records, 'tables>, AcpiRealizationError> {
    let dsdt = AmlDefinitionBlock::from_dsdt(dsdt).map_err(map_aml_error)?;
    let secondary =
        load_secondary_definition_blocks(secondary_definition_tables, definition_storage)?;
    AmlNamespaceLoadPlan::from_definition_blocks(AmlDefinitionBlockSet::new(dsdt, secondary))
        .load_into(namespace_storage)
        .map_err(map_aml_error)
}

/// Loads AML from one validated `DSDT` plus any secondary definition tables, then realizes the
/// matched ACPI backend and activates its AML lifecycle.
pub fn realize_platform_from_definition_tables_with_aml<'records, 'tables, 'issues>(
    fingerprint: &AcpiPlatformFingerprint,
    dsdt: Dsdt<'tables>,
    secondary_definition_tables: &'tables [AcpiTableView<'tables>],
    definition_storage: &'tables mut [MaybeUninit<AmlDefinitionBlock<'tables>>],
    namespace_storage: &'records mut [MaybeUninit<AmlNamespaceLoadRecord>],
    host: &dyn AmlRegionAccessHost,
    runtime: &AmlRuntimeState<'_>,
    issue_storage: &'issues mut [MaybeUninit<AmlBackendVerificationIssue>],
) -> Result<RealizedAcpiPlatformWithAml<'issues>, AcpiRealizationError> {
    let namespace = load_namespace_from_definition_tables(
        dsdt,
        secondary_definition_tables,
        definition_storage,
        namespace_storage,
    )?;
    realize_platform_with_aml(fingerprint, namespace, host, runtime, issue_storage)
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

/// Realizes the Dell Latitude E6430 proving platform and activates AML against one loaded
/// namespace and host/runtime surface.
///
/// # Errors
///
/// Returns one honest error when:
/// - public driver activation fails,
/// - the Dell AML surface does not verify cleanly,
/// - or AML lifecycle activation does not complete cleanly.
pub fn realize_dell_latitude_e6430_platform_with_aml<'records, 'blocks, 'issues>(
    namespace: AmlLoadedNamespace<'records, 'blocks>,
    host: &dyn AmlRegionAccessHost,
    runtime: &AmlRuntimeState<'_>,
    issue_storage: &'issues mut [MaybeUninit<AmlBackendVerificationIssue>],
) -> Result<RealizedAcpiPlatformWithAml<'issues>, AcpiRealizationError> {
    realize_platform_with_aml(
        &dell_latitude_e6430_fingerprint(),
        namespace,
        host,
        runtime,
        issue_storage,
    )
}

/// Dell proving-path wrapper over [`realize_platform_from_definition_tables_with_aml`].
pub fn realize_dell_latitude_e6430_platform_from_definition_tables_with_aml<
    'records,
    'tables,
    'issues,
>(
    dsdt: Dsdt<'tables>,
    secondary_definition_tables: &'tables [AcpiTableView<'tables>],
    definition_storage: &'tables mut [MaybeUninit<AmlDefinitionBlock<'tables>>],
    namespace_storage: &'records mut [MaybeUninit<AmlNamespaceLoadRecord>],
    host: &dyn AmlRegionAccessHost,
    runtime: &AmlRuntimeState<'_>,
    issue_storage: &'issues mut [MaybeUninit<AmlBackendVerificationIssue>],
) -> Result<RealizedAcpiPlatformWithAml<'issues>, AcpiRealizationError> {
    realize_platform_from_definition_tables_with_aml(
        &dell_latitude_e6430_fingerprint(),
        dsdt,
        secondary_definition_tables,
        definition_storage,
        namespace_storage,
        host,
        runtime,
        issue_storage,
    )
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

fn load_secondary_definition_blocks<'tables>(
    secondary_definition_tables: &'tables [AcpiTableView<'tables>],
    definition_storage: &'tables mut [MaybeUninit<AmlDefinitionBlock<'tables>>],
) -> Result<&'tables [AmlDefinitionBlock<'tables>], AcpiRealizationError> {
    if definition_storage.len() < secondary_definition_tables.len() {
        return Err(AcpiRealizationError::resource_exhausted());
    }

    for (index, table) in secondary_definition_tables.iter().copied().enumerate() {
        let block = AmlDefinitionBlock::from_acpi_table(table).map_err(map_aml_error)?;
        definition_storage[index].write(block);
    }

    let secondary = unsafe {
        slice::from_raw_parts(
            definition_storage
                .as_ptr()
                .cast::<AmlDefinitionBlock<'tables>>(),
            secondary_definition_tables.len(),
        )
    };
    Ok(secondary)
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

fn activate_backend_aml<'records, 'blocks, 'issues, B: AcpiAmlBackend>(
    namespace: AmlLoadedNamespace<'records, 'blocks>,
    host: &dyn AmlRegionAccessHost,
    runtime: &AmlRuntimeState<'_>,
    issue_storage: &'issues mut [MaybeUninit<AmlBackendVerificationIssue>],
) -> Result<AcpiAmlActivationReport<'issues>, AcpiRealizationError> {
    let verification =
        verify_acpi_backend::<B>(namespace, 0, issue_storage).map_err(map_aml_error)?;
    if !verification.is_clean() {
        return Err(AcpiRealizationError::invalid());
    }

    let mut vm = AmlVm::default();
    let register_regions = vm
        .register_regions(namespace, host, runtime)
        .map_err(map_aml_error)?;
    if !register_regions.is_clean() {
        return Err(AcpiRealizationError::busy());
    }

    let initialize_devices = vm
        .initialize_devices(namespace, host, runtime)
        .map_err(map_aml_error)?;
    if !initialize_devices.is_clean() {
        return Err(AcpiRealizationError::busy());
    }

    Ok(AcpiAmlActivationReport {
        verification,
        register_regions,
        initialize_devices,
        vm_state: vm.state,
    })
}

fn map_aml_error(error: AmlError) -> AcpiRealizationError {
    match error.kind {
        AmlErrorKind::Unsupported => AcpiRealizationError::unsupported(),
        AmlErrorKind::Overflow => AcpiRealizationError::resource_exhausted(),
        AmlErrorKind::NamespaceConflict | AmlErrorKind::InvalidState => {
            AcpiRealizationError::state_conflict()
        }
        AmlErrorKind::HostFailure => AcpiRealizationError::platform(-1),
        AmlErrorKind::Truncated
        | AmlErrorKind::InvalidBytecode
        | AmlErrorKind::InvalidDefinitionBlock
        | AmlErrorKind::InvalidName
        | AmlErrorKind::InvalidNamespace
        | AmlErrorKind::UndefinedObject => AcpiRealizationError::invalid(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pal::hal::acpi::Dsdt;
    use std::boxed::Box;
    use std::vec::Vec;

    fn definition_block(payload: &'static [u8]) -> Dsdt<'static> {
        let bytes = {
            let mut table = Vec::from([0_u8; 36]);
            table[0..4].copy_from_slice(b"DSDT");
            table[4..8].copy_from_slice(&((36 + payload.len()) as u32).to_le_bytes());
            table[8] = 2;
            table[10..16].copy_from_slice(b"FUSION");
            table[16..24].copy_from_slice(b"ACPIREAL");
            table.extend_from_slice(payload);
            let checksum =
                (!table.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte))).wrapping_add(1);
            table[9] = checksum;
            Box::leak(table.into_boxed_slice())
        };
        Dsdt::parse(bytes).expect("synthetic dsdt should parse")
    }

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

    #[test]
    fn namespace_loads_from_validated_definition_tables() {
        let dsdt = definition_block(&[
            0x10, 0x33, b'\\', b'_', b'S', b'B', b'_', // Scope(\_SB)
            0x08, b'F', b'O', b'O', b'0', 0x0a, 0x01, // Name(FOO0, 1)
            0x14, 0x08, b'_', b'S', b'T', b'A', 0x00, 0xa4, 0x01, // Method(_STA)
            0x5b, 0x80, b'E', b'C', b'O', b'R', 0x03, 0x0a, 0x10, 0x0a, 0x20, // OpRegion
            0x5b, 0x81, 0x10, b'E', b'C', b'O', b'R', 0x01, b'S', b'T', b'0', b'0', 0x08, b'S',
            b'T', b'0', b'1', 0x08, // Field
        ]);
        let mut definition_storage = [];
        let mut namespace_storage = [MaybeUninit::<AmlNamespaceLoadRecord>::uninit(); 16];

        let loaded = load_namespace_from_definition_tables(
            dsdt,
            &[],
            &mut definition_storage,
            &mut namespace_storage,
        )
        .expect("definition-table namespace load should work");

        let foo = crate::aml::AmlResolvedNamePath::parse_text("\\_SB.FOO0")
            .expect("foo path should parse");
        let sta = crate::aml::AmlResolvedNamePath::parse_text("\\_SB._STA")
            .expect("sta path should parse");
        assert!(
            loaded
                .records
                .iter()
                .any(|record| { record.descriptor.path == foo })
        );
        assert!(
            loaded
                .records
                .iter()
                .any(|record| { record.descriptor.path == sta })
        );
    }
}
