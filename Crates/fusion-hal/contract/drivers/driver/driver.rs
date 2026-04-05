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
    pub binding_sources: &'static [DriverBindingSource],
    pub description: &'static str,
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
    len: usize,
}

impl<const CAPACITY: usize> DriverRegistry<CAPACITY> {
    /// Creates one empty driver registry.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            keys: [None; CAPACITY],
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
        binding_sources: &DUMMY_BINDING_SOURCES,
        description: "test-only driver family",
    };

    fn dummy_metadata() -> &'static DriverMetadata {
        &DUMMY_METADATA
    }

    fn enumerate_dummy(
        _registered: &RegisteredDriver<DummyDriver>,
        context: &mut DriverDiscoveryContext<'_>,
        out: &mut [DummyBinding],
    ) -> Result<usize, DriverError> {
        if out.is_empty() {
            return Err(DriverError::resource_exhausted());
        }

        let token = *context.downcast_mut::<u8>()?;
        out[0] = DummyBinding(token);
        Ok(1)
    }

    fn activate_dummy(
        _registered: &RegisteredDriver<DummyDriver>,
        context: &mut DriverActivationContext<'_>,
        binding: DummyBinding,
    ) -> Result<ActiveDriver<DummyDriver>, DriverError> {
        let token = *context.downcast_mut::<u8>()?;
        if binding != DummyBinding(token) {
            return Err(DriverError::invalid());
        }

        Ok(ActiveDriver::new(binding, DummyInstance(token)))
    }

    impl DriverContract for DummyDriver {
        type Binding = DummyBinding;
        type Instance = DummyInstance;

        fn registration() -> DriverRegistration<Self> {
            DriverRegistration::new(
                dummy_metadata,
                DriverActivation::new(enumerate_dummy, activate_dummy),
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
}
