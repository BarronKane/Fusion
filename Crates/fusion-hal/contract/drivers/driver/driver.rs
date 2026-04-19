//! Shared driver registration and activation law.

use core::any::Any;
use core::fmt;
use core::marker::PhantomData;

/// Canonical marketed identity for one driver family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DriverIdentity {
    /// Vendor or silicon house name.
    pub vendor: &'static str,
    /// Optional product family or brand name.
    pub family: Option<&'static str>,
    /// Optional package, chip, or SKU name.
    pub package: Option<&'static str>,
    /// Vendor-marketed product name or class string.
    pub product: &'static str,
    /// Vendor-advertised interface/support string.
    pub advertised_interface: &'static str,
}

impl fmt::Display for DriverIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.vendor)?;

        if let Some(family) = self.family {
            write!(f, " {family}")?;
        }

        if let Some(package) = self.package {
            write!(f, " {package}")?;
        }

        write!(f, " {}", self.product)?;
        write!(f, " ({})", self.advertised_interface)
    }
}

/// Coarse class for one driver family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DriverClass {
    Bus,
    Network,
    Peripheral,
    Display,
    Storage,
    Compute,
    Sensor,
    Other(&'static str),
}

/// Canonical contract-family key implemented by one driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DriverContractKey(pub &'static str);

/// Whether one driver is intrinsically useful on its own or only when something else consumes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DriverUsefulness {
    Standalone,
    MustBeConsumed,
}

/// Enumerated binding/discovery sources a driver can honestly support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DriverBindingSource {
    StaticSoc,
    BoardManifest,
    Acpi,
    Devicetree,
    Pci,
    Usb,
    Sdio,
    Spi,
    I2c,
    Uart,
    Manual,
}

/// Static metadata for one driver family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DriverMetadata {
    pub key: &'static str,
    pub class: DriverClass,
    pub identity: DriverIdentity,
    pub contracts: &'static [DriverContractKey],
    pub required_contracts: &'static [DriverContractKey],
    pub usefulness: DriverUsefulness,
    pub singleton_class: Option<&'static str>,
    pub binding_sources: &'static [DriverBindingSource],
    pub description: &'static str,
}

/// Canonical inactive reason for one registered driver family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DriverInopReason {
    MissingDependency(DriverContractKey),
    Unconsumed,
    SingletonConflict(&'static str),
}

/// Validated readiness state for one registered driver family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DriverAvailability {
    Unknown,
    Ready,
    Inop(DriverInopReason),
}

/// Kind of failure returned by driver registration or activation law.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DriverErrorKind {
    Unsupported,
    Invalid,
    Busy,
    ResourceExhausted,
    StateConflict,
    MissingContext,
    WrongContextType,
    AlreadyRegistered,
    Platform(i32),
}

/// Error returned by driver registration or activation law.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DriverError {
    kind: DriverErrorKind,
}

impl DriverError {
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            kind: DriverErrorKind::Unsupported,
        }
    }

    #[must_use]
    pub const fn invalid() -> Self {
        Self {
            kind: DriverErrorKind::Invalid,
        }
    }

    #[must_use]
    pub const fn busy() -> Self {
        Self {
            kind: DriverErrorKind::Busy,
        }
    }

    #[must_use]
    pub const fn resource_exhausted() -> Self {
        Self {
            kind: DriverErrorKind::ResourceExhausted,
        }
    }

    #[must_use]
    pub const fn state_conflict() -> Self {
        Self {
            kind: DriverErrorKind::StateConflict,
        }
    }

    #[must_use]
    pub const fn missing_context() -> Self {
        Self {
            kind: DriverErrorKind::MissingContext,
        }
    }

    #[must_use]
    pub const fn wrong_context_type() -> Self {
        Self {
            kind: DriverErrorKind::WrongContextType,
        }
    }

    #[must_use]
    pub const fn already_registered() -> Self {
        Self {
            kind: DriverErrorKind::AlreadyRegistered,
        }
    }

    #[must_use]
    pub const fn platform(code: i32) -> Self {
        Self {
            kind: DriverErrorKind::Platform(code),
        }
    }

    #[must_use]
    pub const fn kind(self) -> DriverErrorKind {
        self.kind
    }
}

impl fmt::Display for DriverErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Unsupported => f.write_str("driver operation unsupported"),
            Self::Invalid => f.write_str("invalid driver request"),
            Self::Busy => f.write_str("driver resource busy"),
            Self::ResourceExhausted => f.write_str("driver resources exhausted"),
            Self::StateConflict => f.write_str("driver state conflict"),
            Self::MissingContext => f.write_str("driver activation context missing"),
            Self::WrongContextType => f.write_str("driver activation context type mismatch"),
            Self::AlreadyRegistered => f.write_str("driver already registered"),
            Self::Platform(code) => write!(f, "platform driver error {code}"),
        }
    }
}

impl fmt::Display for DriverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

/// Discovery context owned by the upper-layer enumerator.
pub struct DriverDiscoveryContext<'a> {
    payload: &'a mut dyn Any,
}

impl<'a> DriverDiscoveryContext<'a> {
    /// Creates one discovery context over caller-owned payload.
    #[must_use]
    pub fn new(payload: &'a mut dyn Any) -> Self {
        Self { payload }
    }

    /// Downcasts the discovery payload to the required driver-specific type.
    ///
    /// # Errors
    ///
    /// Returns an error when the payload does not match the expected context type.
    pub fn downcast_mut<T: Any>(&mut self) -> Result<&mut T, DriverError> {
        self.payload
            .downcast_mut::<T>()
            .ok_or_else(DriverError::wrong_context_type)
    }
}

/// Activation context owned by the upper-layer binder.
pub struct DriverActivationContext<'a> {
    payload: &'a mut dyn Any,
}

impl<'a> DriverActivationContext<'a> {
    /// Creates one activation context over caller-owned payload.
    #[must_use]
    pub fn new(payload: &'a mut dyn Any) -> Self {
        Self { payload }
    }

    /// Downcasts the activation payload to the required driver-specific type.
    ///
    /// # Errors
    ///
    /// Returns an error when the payload does not match the expected context type.
    pub fn downcast_mut<T: Any>(&mut self) -> Result<&mut T, DriverError> {
        self.payload
            .downcast_mut::<T>()
            .ok_or_else(DriverError::wrong_context_type)
    }
}

/// Shared law every concrete driver family must implement.
pub trait DriverContract: 'static + Sized {
    type Binding: Copy + Eq;
    type Instance;

    /// Returns the static registration surface for this driver family.
    fn registration() -> DriverRegistration<Self>;
}

/// Static registration surface for one driver family.
pub struct DriverRegistration<D: DriverContract> {
    pub metadata: fn() -> &'static DriverMetadata,
    pub activation: DriverActivation<D>,
}

impl<D: DriverContract> Clone for DriverRegistration<D> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<D: DriverContract> Copy for DriverRegistration<D> {}

impl<D: DriverContract> DriverRegistration<D> {
    /// Creates one static driver registration surface.
    #[must_use]
    pub const fn new(
        metadata: fn() -> &'static DriverMetadata,
        activation: DriverActivation<D>,
    ) -> Self {
        Self {
            metadata,
            activation,
        }
    }
}

/// Static discovery/activation surface for one registered driver family.
pub struct DriverActivation<D: DriverContract> {
    pub enumerate: fn(
        registered: &RegisteredDriver<D>,
        context: &mut DriverDiscoveryContext<'_>,
        out: &mut [D::Binding],
    ) -> Result<usize, DriverError>,
    pub activate: fn(
        registered: &RegisteredDriver<D>,
        context: &mut DriverActivationContext<'_>,
        binding: D::Binding,
    ) -> Result<ActiveDriver<D>, DriverError>,
    marker: PhantomData<fn() -> D>,
}

impl<D: DriverContract> Clone for DriverActivation<D> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<D: DriverContract> Copy for DriverActivation<D> {}

impl<D: DriverContract> DriverActivation<D> {
    /// Creates one static activation surface for one driver family.
    #[must_use]
    pub const fn new(
        enumerate: fn(
            registered: &RegisteredDriver<D>,
            context: &mut DriverDiscoveryContext<'_>,
            out: &mut [D::Binding],
        ) -> Result<usize, DriverError>,
        activate: fn(
            registered: &RegisteredDriver<D>,
            context: &mut DriverActivationContext<'_>,
            binding: D::Binding,
        ) -> Result<ActiveDriver<D>, DriverError>,
    ) -> Self {
        Self {
            enumerate,
            activate,
            marker: PhantomData,
        }
    }
}

/// Registration proof for one driver family.
pub struct RegisteredDriver<D: DriverContract> {
    slot: usize,
    registration: DriverRegistration<D>,
}

impl<D: DriverContract> RegisteredDriver<D> {
    /// Returns the truthful static metadata for this registered driver family.
    #[must_use]
    pub fn metadata(&self) -> &'static DriverMetadata {
        (self.registration.metadata)()
    }

    /// Returns the canonical driver key for this registered family.
    #[must_use]
    pub fn key(&self) -> &'static str {
        self.metadata().key
    }

    /// Returns the internal registry slot used for this registration.
    #[must_use]
    pub const fn slot(&self) -> usize {
        self.slot
    }

    /// Enumerates driver bindings through the registered activation surface.
    pub fn enumerate_bindings(
        &self,
        context: &mut DriverDiscoveryContext<'_>,
        out: &mut [D::Binding],
    ) -> Result<usize, DriverError> {
        (self.registration.activation.enumerate)(self, context, out)
    }

    /// Activates one bound driver instance through the registered activation surface.
    pub fn activate(
        &self,
        context: &mut DriverActivationContext<'_>,
        binding: D::Binding,
    ) -> Result<ActiveDriver<D>, DriverError> {
        (self.registration.activation.activate)(self, context, binding)
    }
}

/// Active, functioning instance of one registered driver family.
pub struct ActiveDriver<D: DriverContract> {
    binding: D::Binding,
    instance: D::Instance,
}

impl<D: DriverContract> ActiveDriver<D> {
    /// Creates one active driver instance from one registered binding.
    #[must_use]
    pub const fn new(binding: D::Binding, instance: D::Instance) -> Self {
        Self { binding, instance }
    }

    /// Returns the binding used to activate this driver instance.
    #[must_use]
    pub const fn binding(&self) -> D::Binding {
        self.binding
    }

    /// Returns a shared reference to the active driver instance.
    #[must_use]
    pub fn instance(&self) -> &D::Instance {
        &self.instance
    }

    /// Returns a mutable reference to the active driver instance.
    #[must_use]
    pub fn instance_mut(&mut self) -> &mut D::Instance {
        &mut self.instance
    }

    /// Consumes the activation wrapper and returns the live driver instance.
    #[must_use]
    pub fn into_instance(self) -> D::Instance {
        self.instance
    }
}

/// Fixed-capacity driver registry that mints registration proof tokens.
pub struct DriverRegistry<const CAPACITY: usize> {
    keys: [Option<&'static str>; CAPACITY],
    metadata: [Option<&'static DriverMetadata>; CAPACITY],
    states: [DriverAvailability; CAPACITY],
    len: usize,
}

impl<const CAPACITY: usize> DriverRegistry<CAPACITY> {
    /// Creates one empty driver registry.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            keys: [None; CAPACITY],
            metadata: [None; CAPACITY],
            states: [DriverAvailability::Unknown; CAPACITY],
            len: 0,
        }
    }

    /// Returns the number of registered drivers currently tracked.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Returns whether the registry is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns whether one canonical driver key is already registered.
    #[must_use]
    pub fn contains_key(&self, key: &'static str) -> bool {
        self.keys[..self.len]
            .iter()
            .flatten()
            .any(|registered| *registered == key)
    }

    /// Returns whether one contract key is exported by any registered driver family.
    #[must_use]
    pub fn contains_contract(&self, contract: DriverContractKey) -> bool {
        self.metadata[..self.len]
            .iter()
            .flatten()
            .any(|metadata| metadata.contracts.contains(&contract))
    }

    /// Returns the validated availability state for one registered driver proof token.
    #[must_use]
    pub fn state<D: DriverContract>(&self, registered: &RegisteredDriver<D>) -> DriverAvailability {
        self.states[registered.slot()]
    }

    /// Returns the validated availability state for one driver key when registered.
    #[must_use]
    pub fn state_for_key(&self, key: &'static str) -> Option<DriverAvailability> {
        self.keys[..self.len]
            .iter()
            .position(|registered| registered.is_some_and(|registered| registered == key))
            .map(|slot| self.states[slot])
    }

    /// Recomputes validated driver readiness across all registered families.
    ///
    /// Drivers become:
    /// - `Ready` when their requirements are satisfied, they do not violate singleton authority,
    ///   and they are either intrinsically useful or consumed by another ready driver.
    /// - `Inop(...)` otherwise.
    pub fn validate(&mut self) {
        for state in &mut self.states[..self.len] {
            *state = DriverAvailability::Ready;
        }

        loop {
            let previous = self.states;
            let mut changed = false;

            for slot in 0..self.len {
                let metadata = self.metadata[slot].expect("registered metadata");
                let desired = self.compute_state(slot, &previous, metadata);
                if self.states[slot] != desired {
                    self.states[slot] = desired;
                    changed = true;
                }
            }

            if !changed {
                break;
            }
        }
    }

    fn compute_state(
        &self,
        slot: usize,
        states: &[DriverAvailability; CAPACITY],
        metadata: &'static DriverMetadata,
    ) -> DriverAvailability {
        if let Some(singleton_class) = metadata.singleton_class {
            if self.has_prior_singleton_conflict(slot, singleton_class) {
                return DriverAvailability::Inop(DriverInopReason::SingletonConflict(
                    singleton_class,
                ));
            }
        }

        for required in metadata.required_contracts {
            if !self.has_ready_contract_provider(slot, states, *required) {
                return DriverAvailability::Inop(DriverInopReason::MissingDependency(*required));
            }
        }

        if metadata.usefulness == DriverUsefulness::MustBeConsumed
            && !self.has_ready_consumer(slot, states, metadata.contracts)
        {
            return DriverAvailability::Inop(DriverInopReason::Unconsumed);
        }

        DriverAvailability::Ready
    }

    fn has_prior_singleton_conflict(&self, slot: usize, singleton_class: &'static str) -> bool {
        self.metadata[..self.len]
            .iter()
            .take(slot)
            .flatten()
            .any(|metadata| metadata.singleton_class == Some(singleton_class))
    }

    fn has_ready_contract_provider(
        &self,
        slot: usize,
        states: &[DriverAvailability; CAPACITY],
        required: DriverContractKey,
    ) -> bool {
        self.metadata[..self.len]
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != slot)
            .filter_map(|(index, metadata)| metadata.map(|metadata| (index, metadata)))
            .any(|(index, metadata)| {
                states[index] == DriverAvailability::Ready && metadata.contracts.contains(&required)
            })
    }

    fn has_ready_consumer(
        &self,
        slot: usize,
        states: &[DriverAvailability; CAPACITY],
        exported_contracts: &'static [DriverContractKey],
    ) -> bool {
        self.metadata[..self.len]
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != slot)
            .filter_map(|(index, metadata)| metadata.map(|metadata| (index, metadata)))
            .any(|(index, metadata)| {
                states[index] == DriverAvailability::Ready
                    && metadata
                        .required_contracts
                        .iter()
                        .any(|required| exported_contracts.contains(required))
            })
    }

    /// Registers one driver family and returns the proof token required for activation.
    ///
    /// # Errors
    ///
    /// Returns an error when the driver is already registered or the registry is full.
    pub fn register<D: DriverContract>(&mut self) -> Result<RegisteredDriver<D>, DriverError> {
        let registration = D::registration();
        let metadata = (registration.metadata)();

        if self.contains_key(metadata.key) {
            return Err(DriverError::already_registered());
        }

        if self.len == CAPACITY {
            return Err(DriverError::resource_exhausted());
        }

        let slot = self.len;
        self.keys[slot] = Some(metadata.key);
        self.metadata[slot] = Some(metadata);
        self.states[slot] = DriverAvailability::Unknown;
        self.len += 1;

        Ok(RegisteredDriver { slot, registration })
    }
}

impl<const CAPACITY: usize> Default for DriverRegistry<CAPACITY> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct DummyBinding(u8);

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct DummyInstance(u8);

    struct DummyDriver;

    const DUMMY_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("test.dummy")];
    const DUMMY_REQUIRED_CONTRACTS: [DriverContractKey; 0] = [];
    const DUMMY_BINDING_SOURCES: [DriverBindingSource; 1] = [DriverBindingSource::Manual];
    const DUMMY_METADATA: DriverMetadata = DriverMetadata {
        key: "test.dummy.driver",
        class: DriverClass::Other("test"),
        identity: DriverIdentity {
            vendor: "Fusion",
            family: Some("Test"),
            package: None,
            product: "Dummy driver",
            advertised_interface: "test",
        },
        contracts: &DUMMY_CONTRACTS,
        required_contracts: &DUMMY_REQUIRED_CONTRACTS,
        usefulness: DriverUsefulness::Standalone,
        singleton_class: None,
        binding_sources: &DUMMY_BINDING_SOURCES,
        description: "test-only driver family",
    };

    struct RootDriver;
    struct LeafDriver;
    struct MiddleDriver;
    struct TopDriver;
    struct LayoutDriver;
    struct OtherLayoutDriver;
    struct PortDriver;

    const ROOT_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("test.root")];
    const ROOT_REQUIRED_CONTRACTS: [DriverContractKey; 0] = [];
    const LEAF_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("test.leaf")];
    const LEAF_REQUIRED_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("test.root")];
    const MIDDLE_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("test.middle")];
    const MIDDLE_REQUIRED_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("test.root")];
    const TOP_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("test.top")];
    const TOP_REQUIRED_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("test.middle")];
    const LAYOUT_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("display.layout")];
    const LAYOUT_REQUIRED_CONTRACTS: [DriverContractKey; 0] = [];
    const PORT_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("display.port")];
    const PORT_REQUIRED_CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("display.layout")];

    const ROOT_METADATA: DriverMetadata = DriverMetadata {
        key: "test.root.driver",
        class: DriverClass::Other("test"),
        identity: DriverIdentity {
            vendor: "Fusion",
            family: Some("Test"),
            package: None,
            product: "Root driver",
            advertised_interface: "test root",
        },
        contracts: &ROOT_CONTRACTS,
        required_contracts: &ROOT_REQUIRED_CONTRACTS,
        usefulness: DriverUsefulness::Standalone,
        singleton_class: None,
        binding_sources: &DUMMY_BINDING_SOURCES,
        description: "test root driver family",
    };

    const LEAF_METADATA: DriverMetadata = DriverMetadata {
        key: "test.leaf.driver",
        class: DriverClass::Other("test"),
        identity: DriverIdentity {
            vendor: "Fusion",
            family: Some("Test"),
            package: None,
            product: "Leaf driver",
            advertised_interface: "test leaf",
        },
        contracts: &LEAF_CONTRACTS,
        required_contracts: &LEAF_REQUIRED_CONTRACTS,
        usefulness: DriverUsefulness::Standalone,
        singleton_class: None,
        binding_sources: &DUMMY_BINDING_SOURCES,
        description: "test leaf driver family",
    };

    const MIDDLE_METADATA: DriverMetadata = DriverMetadata {
        key: "test.middle.driver",
        class: DriverClass::Other("test"),
        identity: DriverIdentity {
            vendor: "Fusion",
            family: Some("Test"),
            package: None,
            product: "Middle driver",
            advertised_interface: "test middle",
        },
        contracts: &MIDDLE_CONTRACTS,
        required_contracts: &MIDDLE_REQUIRED_CONTRACTS,
        usefulness: DriverUsefulness::Standalone,
        singleton_class: None,
        binding_sources: &DUMMY_BINDING_SOURCES,
        description: "test middle driver family",
    };

    const TOP_METADATA: DriverMetadata = DriverMetadata {
        key: "test.top.driver",
        class: DriverClass::Other("test"),
        identity: DriverIdentity {
            vendor: "Fusion",
            family: Some("Test"),
            package: None,
            product: "Top driver",
            advertised_interface: "test top",
        },
        contracts: &TOP_CONTRACTS,
        required_contracts: &TOP_REQUIRED_CONTRACTS,
        usefulness: DriverUsefulness::Standalone,
        singleton_class: None,
        binding_sources: &DUMMY_BINDING_SOURCES,
        description: "test top driver family",
    };

    const LAYOUT_METADATA: DriverMetadata = DriverMetadata {
        key: "display.layout.test",
        class: DriverClass::Display,
        identity: DriverIdentity {
            vendor: "Fusion",
            family: Some("Display"),
            package: None,
            product: "Test layout driver",
            advertised_interface: "test layout",
        },
        contracts: &LAYOUT_CONTRACTS,
        required_contracts: &LAYOUT_REQUIRED_CONTRACTS,
        usefulness: DriverUsefulness::MustBeConsumed,
        singleton_class: Some("display.layout.machine"),
        binding_sources: &DUMMY_BINDING_SOURCES,
        description: "test display layout family",
    };

    const OTHER_LAYOUT_METADATA: DriverMetadata = DriverMetadata {
        key: "display.layout.other",
        class: DriverClass::Display,
        identity: DriverIdentity {
            vendor: "Fusion",
            family: Some("Display"),
            package: None,
            product: "Other test layout driver",
            advertised_interface: "other test layout",
        },
        contracts: &LAYOUT_CONTRACTS,
        required_contracts: &LAYOUT_REQUIRED_CONTRACTS,
        usefulness: DriverUsefulness::MustBeConsumed,
        singleton_class: Some("display.layout.machine"),
        binding_sources: &DUMMY_BINDING_SOURCES,
        description: "other test display layout family",
    };

    const PORT_METADATA: DriverMetadata = DriverMetadata {
        key: "display.port.test",
        class: DriverClass::Display,
        identity: DriverIdentity {
            vendor: "Fusion",
            family: Some("Display"),
            package: None,
            product: "Test port driver",
            advertised_interface: "test port",
        },
        contracts: &PORT_CONTRACTS,
        required_contracts: &PORT_REQUIRED_CONTRACTS,
        usefulness: DriverUsefulness::Standalone,
        singleton_class: None,
        binding_sources: &DUMMY_BINDING_SOURCES,
        description: "test display port family",
    };

    fn dummy_metadata() -> &'static DriverMetadata {
        &DUMMY_METADATA
    }

    fn enumerate_test_driver<D>(
        _registered: &RegisteredDriver<D>,
        context: &mut DriverDiscoveryContext<'_>,
        out: &mut [DummyBinding],
    ) -> Result<usize, DriverError>
    where
        D: DriverContract<Binding = DummyBinding, Instance = DummyInstance>,
    {
        if out.is_empty() {
            return Err(DriverError::resource_exhausted());
        }

        let token = *context.downcast_mut::<u8>()?;
        out[0] = DummyBinding(token);
        Ok(1)
    }

    fn activate_test_driver<D>(
        _registered: &RegisteredDriver<D>,
        context: &mut DriverActivationContext<'_>,
        binding: DummyBinding,
    ) -> Result<ActiveDriver<D>, DriverError>
    where
        D: DriverContract<Binding = DummyBinding, Instance = DummyInstance>,
    {
        let token = *context.downcast_mut::<u8>()?;
        if binding != DummyBinding(token) {
            return Err(DriverError::invalid());
        }

        Ok(ActiveDriver::new(binding, DummyInstance(token)))
    }

    fn root_metadata() -> &'static DriverMetadata {
        &ROOT_METADATA
    }

    fn leaf_metadata() -> &'static DriverMetadata {
        &LEAF_METADATA
    }

    fn layout_metadata() -> &'static DriverMetadata {
        &LAYOUT_METADATA
    }

    fn other_layout_metadata() -> &'static DriverMetadata {
        &OTHER_LAYOUT_METADATA
    }

    fn port_metadata() -> &'static DriverMetadata {
        &PORT_METADATA
    }

    fn middle_metadata() -> &'static DriverMetadata {
        &MIDDLE_METADATA
    }

    fn top_metadata() -> &'static DriverMetadata {
        &TOP_METADATA
    }

    impl DriverContract for DummyDriver {
        type Binding = DummyBinding;
        type Instance = DummyInstance;

        fn registration() -> DriverRegistration<Self> {
            DriverRegistration::new(
                dummy_metadata,
                DriverActivation::new(enumerate_test_driver::<Self>, activate_test_driver::<Self>),
            )
        }
    }

    impl DriverContract for RootDriver {
        type Binding = DummyBinding;
        type Instance = DummyInstance;

        fn registration() -> DriverRegistration<Self> {
            DriverRegistration::new(
                root_metadata,
                DriverActivation::new(enumerate_test_driver::<Self>, activate_test_driver::<Self>),
            )
        }
    }

    impl DriverContract for LeafDriver {
        type Binding = DummyBinding;
        type Instance = DummyInstance;

        fn registration() -> DriverRegistration<Self> {
            DriverRegistration::new(
                leaf_metadata,
                DriverActivation::new(enumerate_test_driver::<Self>, activate_test_driver::<Self>),
            )
        }
    }

    impl DriverContract for LayoutDriver {
        type Binding = DummyBinding;
        type Instance = DummyInstance;

        fn registration() -> DriverRegistration<Self> {
            DriverRegistration::new(
                layout_metadata,
                DriverActivation::new(enumerate_test_driver::<Self>, activate_test_driver::<Self>),
            )
        }
    }

    impl DriverContract for MiddleDriver {
        type Binding = DummyBinding;
        type Instance = DummyInstance;

        fn registration() -> DriverRegistration<Self> {
            DriverRegistration::new(
                middle_metadata,
                DriverActivation::new(enumerate_test_driver::<Self>, activate_test_driver::<Self>),
            )
        }
    }

    impl DriverContract for OtherLayoutDriver {
        type Binding = DummyBinding;
        type Instance = DummyInstance;

        fn registration() -> DriverRegistration<Self> {
            DriverRegistration::new(
                other_layout_metadata,
                DriverActivation::new(enumerate_test_driver::<Self>, activate_test_driver::<Self>),
            )
        }
    }

    impl DriverContract for TopDriver {
        type Binding = DummyBinding;
        type Instance = DummyInstance;

        fn registration() -> DriverRegistration<Self> {
            DriverRegistration::new(
                top_metadata,
                DriverActivation::new(enumerate_test_driver::<Self>, activate_test_driver::<Self>),
            )
        }
    }

    impl DriverContract for PortDriver {
        type Binding = DummyBinding;
        type Instance = DummyInstance;

        fn registration() -> DriverRegistration<Self> {
            DriverRegistration::new(
                port_metadata,
                DriverActivation::new(enumerate_test_driver::<Self>, activate_test_driver::<Self>),
            )
        }
    }

    #[test]
    fn registry_rejects_duplicate_driver_registration() {
        let mut registry = DriverRegistry::<1>::new();
        let registered = registry
            .register::<DummyDriver>()
            .expect("dummy driver should register");
        assert_eq!(registered.key(), "test.dummy.driver");
        assert_eq!(registry.len(), 1);
        match registry.register::<DummyDriver>() {
            Ok(_) => panic!("duplicate driver registration should fail"),
            Err(error) => assert_eq!(error.kind(), DriverErrorKind::AlreadyRegistered),
        }
    }

    #[test]
    fn registered_driver_enumerates_and_activates() {
        let mut registry = DriverRegistry::<1>::new();
        let registered = registry
            .register::<DummyDriver>()
            .expect("dummy driver should register");

        let mut discovery_token = 7_u8;
        let mut discovery = DriverDiscoveryContext::new(&mut discovery_token);
        let mut bindings = [DummyBinding(0)];
        let count = registered
            .enumerate_bindings(&mut discovery, &mut bindings)
            .expect("enumeration should succeed");
        assert_eq!(count, 1);
        assert_eq!(bindings[0], DummyBinding(7));

        let mut activation_token = 7_u8;
        let mut activation = DriverActivationContext::new(&mut activation_token);
        let active = registered
            .activate(&mut activation, bindings[0])
            .expect("activation should succeed");
        assert_eq!(active.binding(), DummyBinding(7));
        assert_eq!(*active.instance(), DummyInstance(7));
    }

    #[test]
    fn wrong_context_type_is_reported_honestly() {
        let mut registry = DriverRegistry::<1>::new();
        let registered = registry
            .register::<DummyDriver>()
            .expect("dummy driver should register");

        let mut wrong_context = ();
        let mut discovery = DriverDiscoveryContext::new(&mut wrong_context);
        let mut bindings = [DummyBinding(0)];
        assert_eq!(
            registered
                .enumerate_bindings(&mut discovery, &mut bindings)
                .unwrap_err()
                .kind(),
            DriverErrorKind::WrongContextType
        );
    }

    #[test]
    fn registry_marks_missing_dependency_as_inop() {
        let mut registry = DriverRegistry::<1>::new();
        let leaf = registry
            .register::<LeafDriver>()
            .expect("leaf driver should register");

        registry.validate();

        assert_eq!(
            registry.state(&leaf),
            DriverAvailability::Inop(DriverInopReason::MissingDependency(DriverContractKey(
                "test.root"
            ),))
        );
    }

    #[test]
    fn registry_marks_satisfied_dependency_ready() {
        let mut registry = DriverRegistry::<2>::new();
        let root = registry
            .register::<RootDriver>()
            .expect("root driver should register");
        let leaf = registry
            .register::<LeafDriver>()
            .expect("leaf driver should register");

        registry.validate();

        assert_eq!(registry.state(&root), DriverAvailability::Ready);
        assert_eq!(registry.state(&leaf), DriverAvailability::Ready);
    }

    #[test]
    fn registry_cascades_inop_through_dependency_chain() {
        let mut registry = DriverRegistry::<2>::new();
        let middle = registry
            .register::<MiddleDriver>()
            .expect("middle driver should register");
        let top = registry
            .register::<TopDriver>()
            .expect("top driver should register");

        registry.validate();

        assert_eq!(
            registry.state(&middle),
            DriverAvailability::Inop(DriverInopReason::MissingDependency(DriverContractKey(
                "test.root"
            ),))
        );
        assert_eq!(
            registry.state(&top),
            DriverAvailability::Inop(DriverInopReason::MissingDependency(DriverContractKey(
                "test.middle"
            ),))
        );
    }

    #[test]
    fn registry_marks_consumed_layout_ready() {
        let mut registry = DriverRegistry::<2>::new();
        let layout = registry
            .register::<LayoutDriver>()
            .expect("layout driver should register");
        let port = registry
            .register::<PortDriver>()
            .expect("port driver should register");

        registry.validate();

        assert_eq!(registry.state(&layout), DriverAvailability::Ready);
        assert_eq!(registry.state(&port), DriverAvailability::Ready);
    }

    #[test]
    fn registry_marks_unconsumed_transient_driver_inop() {
        let mut registry = DriverRegistry::<1>::new();
        let layout = registry
            .register::<LayoutDriver>()
            .expect("layout driver should register");

        registry.validate();

        assert_eq!(
            registry.state(&layout),
            DriverAvailability::Inop(DriverInopReason::Unconsumed)
        );
    }

    #[test]
    fn registry_marks_singleton_conflict_inop() {
        let mut registry = DriverRegistry::<3>::new();
        let layout = registry
            .register::<LayoutDriver>()
            .expect("first layout driver should register");
        let port = registry
            .register::<PortDriver>()
            .expect("port driver should register");
        let other = registry
            .register::<OtherLayoutDriver>()
            .expect("second layout driver should register");

        registry.validate();

        assert_eq!(registry.state(&layout), DriverAvailability::Ready);
        assert_eq!(registry.state(&port), DriverAvailability::Ready);
        assert_eq!(
            registry.state(&other),
            DriverAvailability::Inop(DriverInopReason::SingletonConflict(
                "display.layout.machine",
            ))
        );
    }
}
