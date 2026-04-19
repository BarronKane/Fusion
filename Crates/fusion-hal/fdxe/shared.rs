// Shared FDXE ABI body.
//
// This file is the single source of truth for the FDXE layout and is consumed in four places:
// - directly by `fusion-hal::fdxe`
// - staged into `OUT_DIR` by `fusion-firmware/build.rs`
// - staged into `OUT_DIR` by the GPIO driver crate `build.rs`
// - staged into `OUT_DIR` by the CYW43439 driver crate `build.rs`
//
// If this file moves, update the three build scripts and `fusion-hal/fdxe/fdxe.rs` together.

use core::marker::PhantomData;
use core::mem::{
    MaybeUninit,
    size_of,
};
use core::slice;
use core::str;

/// Exported symbol name for one version-1 FDXE module header.
pub const FDXE_MODULE_V1_SYMBOL_NAME: &str = "fdxe_module_v1";
/// Null-terminated exported symbol name for dynamic loader consumers.
pub const FDXE_MODULE_V1_SYMBOL_NAME_CSTR: &[u8] = b"fdxe_module_v1\0";
/// Linker section used for statically embedded FDXE module records.
pub const FDXE_STATIC_MODULE_SECTION_NAME: &str = ".fdxe.modules";
/// Magic tag for one version-1 FDXE module header.
pub const FDXE_MODULE_V1_MAGIC: [u8; 8] = *b"FDXE0001";
/// Current FDXE module ABI version.
pub const FDXE_MODULE_V1_ABI_VERSION: u32 = 1;
/// Little-endian layout tag.
pub const FDXE_ENDIANNESS_LITTLE: u8 = 1;
/// Big-endian layout tag.
pub const FDXE_ENDIANNESS_BIG: u8 = 2;
/// Platform error code surfaced when one requested FDXE module carries the wrong magic tag.
pub const FDXE_DRIVER_PLATFORM_BAD_MAGIC: i32 = -12_001;
/// Platform error code surfaced when one requested FDXE module uses the wrong ABI version.
pub const FDXE_DRIVER_PLATFORM_ABI_MISMATCH: i32 = -12_002;
/// Platform error code surfaced when one requested FDXE module layout is malformed.
pub const FDXE_DRIVER_PLATFORM_LAYOUT_MISMATCH: i32 = -12_003;

/// One ABI-facing string slice.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FdxeStr {
    pub ptr: *const u8,
    pub len: usize,
}

// SAFETY: `FdxeStr` is only constructed from static module image strings and is immutable.
unsafe impl Sync for FdxeStr {}

impl FdxeStr {
    /// Creates one ABI-facing string slice from a static Rust string.
    #[must_use]
    pub const fn new(value: &'static str) -> Self {
        Self {
            ptr: value.as_ptr(),
            len: value.len(),
        }
    }

    /// Returns this ABI-facing string as a static Rust string.
    ///
    /// # Safety
    ///
    /// The caller must ensure the pointer/length pair originated from a valid static UTF-8
    /// string in the loaded module image.
    #[must_use]
    pub unsafe fn as_str(self) -> &'static str {
        let bytes = unsafe { slice::from_raw_parts(self.ptr, self.len) };
        unsafe { str::from_utf8_unchecked(bytes) }
    }
}

impl Default for FdxeStr {
    fn default() -> Self {
        Self::new("")
    }
}

/// One exported driver family entry in an FDXE module.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FdxeDriverExportV1 {
    pub struct_size: usize,
    pub driver_key: FdxeStr,
    pub metadata: fn() -> &'static DriverMetadata,
}

// SAFETY: exported driver entries are immutable static module records.
unsafe impl Sync for FdxeDriverExportV1 {}

impl FdxeDriverExportV1 {
    /// Creates one exported driver family entry.
    #[must_use]
    pub const fn new(driver_key: &'static str, metadata: fn() -> &'static DriverMetadata) -> Self {
        Self {
            struct_size: size_of::<Self>(),
            driver_key: FdxeStr::new(driver_key),
            metadata,
        }
    }

    /// Returns the truthful static metadata for this exported driver family.
    #[must_use]
    pub fn metadata(&self) -> &'static DriverMetadata {
        (self.metadata)()
    }

    /// Returns the exported canonical driver key.
    #[must_use]
    pub fn driver_key(&self) -> &'static str {
        self.metadata().key
    }
}

/// One statically embedded FDXE module record placed into a linker section.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FdxeStaticModuleV1 {
    pub struct_size: usize,
    pub module: *const FdxeModuleV1,
}

// SAFETY: embedded static module records are immutable linker-visible records.
unsafe impl Sync for FdxeStaticModuleV1 {}

impl FdxeStaticModuleV1 {
    /// Creates one statically embedded FDXE module record.
    #[must_use]
    pub const fn new(module: &'static FdxeModuleV1) -> Self {
        Self {
            struct_size: size_of::<Self>(),
            module: module as *const FdxeModuleV1,
        }
    }
}

/// One exported version-1 FDXE module header.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FdxeModuleV1 {
    pub magic: [u8; 8],
    pub abi_version: u32,
    pub struct_size: usize,
    pub export_struct_size: usize,
    pub driver_metadata_size: usize,
    pub pointer_width_bits: u8,
    pub endianness: u8,
    pub reserved: [u8; 6],
    pub module_name: FdxeStr,
    pub target_name: FdxeStr,
    pub driver_count: usize,
    pub drivers: *const FdxeDriverExportV1,
}

// SAFETY: module headers are immutable static records pointing at immutable exported slices.
unsafe impl Sync for FdxeModuleV1 {}

impl FdxeModuleV1 {
    /// Creates one exported version-1 FDXE module header.
    ///
    /// `module_name` is the stable identity of the driver-module crate itself.
    ///
    /// `target_name` is the build/profile/board target identity that this concrete module image
    /// was produced for. These are intentionally distinct even when early modules happen to use
    /// very similar strings.
    #[must_use]
    pub const fn new(
        module_name: &'static str,
        target_name: &'static str,
        drivers: &'static [FdxeDriverExportV1],
    ) -> Self {
        Self {
            magic: FDXE_MODULE_V1_MAGIC,
            abi_version: FDXE_MODULE_V1_ABI_VERSION,
            struct_size: size_of::<Self>(),
            export_struct_size: size_of::<FdxeDriverExportV1>(),
            driver_metadata_size: size_of::<DriverMetadata>(),
            pointer_width_bits: usize::BITS as u8,
            endianness: if cfg!(target_endian = "little") {
                FDXE_ENDIANNESS_LITTLE
            } else {
                FDXE_ENDIANNESS_BIG
            },
            reserved: [0; 6],
            module_name: FdxeStr::new(module_name),
            target_name: FdxeStr::new(target_name),
            driver_count: drivers.len(),
            drivers: drivers.as_ptr(),
        }
    }

    /// Validates this header against the in-process firmware ABI expectations.
    ///
    /// # Errors
    ///
    /// Returns an error if the loaded module header is not layout-compatible with the current
    /// firmware image.
    pub fn validate(&self) -> Result<(), FdxeModuleError> {
        if self.magic != FDXE_MODULE_V1_MAGIC {
            return Err(FdxeModuleError::bad_magic());
        }
        if self.abi_version != FDXE_MODULE_V1_ABI_VERSION {
            return Err(FdxeModuleError::abi_mismatch());
        }
        if self.struct_size != size_of::<Self>() {
            return Err(FdxeModuleError::layout_mismatch());
        }
        if self.export_struct_size != size_of::<FdxeDriverExportV1>() {
            return Err(FdxeModuleError::layout_mismatch());
        }
        if self.driver_metadata_size != size_of::<DriverMetadata>() {
            return Err(FdxeModuleError::layout_mismatch());
        }
        if self.pointer_width_bits != usize::BITS as u8 {
            return Err(FdxeModuleError::layout_mismatch());
        }

        let expected_endianness = if cfg!(target_endian = "little") {
            FDXE_ENDIANNESS_LITTLE
        } else {
            FDXE_ENDIANNESS_BIG
        };

        if self.endianness != expected_endianness {
            return Err(FdxeModuleError::layout_mismatch());
        }

        Ok(())
    }

    /// Returns the exported driver list after validating the header.
    ///
    /// # Errors
    ///
    /// Returns an error if the module header is not valid for the current firmware image.
    pub fn drivers(&self) -> Result<&'static [FdxeDriverExportV1], FdxeModuleError> {
        self.validate()?;
        // SAFETY: the header has been validated and the slice is module-owned static data.
        let drivers = unsafe { slice::from_raw_parts(self.drivers, self.driver_count) };

        for driver in drivers {
            if driver.struct_size != size_of::<FdxeDriverExportV1>() {
                return Err(FdxeModuleError::layout_mismatch());
            }
        }

        Ok(drivers)
    }

    /// Returns the stable exported module identity string after validating the header.
    ///
    /// # Errors
    ///
    /// Returns an error if the module header is not valid for the current firmware image.
    pub fn module_name(&self) -> Result<&'static str, FdxeModuleError> {
        self.validate()?;
        // SAFETY: validated FDXE headers only expose static UTF-8 strings built into the module.
        Ok(unsafe { self.module_name.as_str() })
    }

    /// Returns the concrete target/profile identity string after validating the header.
    ///
    /// # Errors
    ///
    /// Returns an error if the module header is not valid for the current firmware image.
    pub fn target_name(&self) -> Result<&'static str, FdxeModuleError> {
        self.validate()?;
        // SAFETY: validated FDXE headers only expose static UTF-8 strings built into the module.
        Ok(unsafe { self.target_name.as_str() })
    }
}

/// Error returned by FDXE module validation or registry population.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FdxeModuleError {
    kind: FdxeModuleErrorKind,
}

impl FdxeModuleError {
    #[must_use]
    pub const fn bad_magic() -> Self {
        Self {
            kind: FdxeModuleErrorKind::BadMagic,
        }
    }

    #[must_use]
    pub const fn abi_mismatch() -> Self {
        Self {
            kind: FdxeModuleErrorKind::AbiMismatch,
        }
    }

    #[must_use]
    pub const fn layout_mismatch() -> Self {
        Self {
            kind: FdxeModuleErrorKind::LayoutMismatch,
        }
    }

    #[must_use]
    pub const fn duplicate_module() -> Self {
        Self {
            kind: FdxeModuleErrorKind::DuplicateModule,
        }
    }

    #[must_use]
    pub const fn capacity_exhausted() -> Self {
        Self {
            kind: FdxeModuleErrorKind::CapacityExhausted,
        }
    }

    #[must_use]
    pub const fn kind(self) -> FdxeModuleErrorKind {
        self.kind
    }
}

impl From<FdxeModuleError> for DriverError {
    fn from(error: FdxeModuleError) -> Self {
        match error.kind() {
            FdxeModuleErrorKind::BadMagic => DriverError::platform(FDXE_DRIVER_PLATFORM_BAD_MAGIC),
            FdxeModuleErrorKind::AbiMismatch => {
                DriverError::platform(FDXE_DRIVER_PLATFORM_ABI_MISMATCH)
            }
            FdxeModuleErrorKind::LayoutMismatch => {
                DriverError::platform(FDXE_DRIVER_PLATFORM_LAYOUT_MISMATCH)
            }
            FdxeModuleErrorKind::DuplicateModule => DriverError::already_registered(),
            FdxeModuleErrorKind::CapacityExhausted => DriverError::resource_exhausted(),
        }
    }
}

/// Kind of failure returned by FDXE module validation or registry population.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FdxeModuleErrorKind {
    BadMagic,
    AbiMismatch,
    LayoutMismatch,
    DuplicateModule,
    CapacityExhausted,
}

/// Counted inventory for one validated FDXE module set before registry population.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct FdxeModuleInventory {
    pub module_count: usize,
    pub driver_count: usize,
}

fn resolve_static_module(entry: &FdxeStaticModuleV1) -> Result<&'static FdxeModuleV1, FdxeModuleError> {
    if entry.struct_size != size_of::<FdxeStaticModuleV1>() {
        return Err(FdxeModuleError::layout_mismatch());
    }

    let Some(module) = (unsafe { entry.module.as_ref() }) else {
        return Err(FdxeModuleError::layout_mismatch());
    };

    Ok(module)
}

/// Inventories one caller-supplied slice of statically embedded module records.
///
/// # Errors
///
/// Returns an error if any record is malformed or any embedded module fails normal FDXE
/// validation.
pub fn inventory_static_modules(
    modules: &[FdxeStaticModuleV1],
) -> Result<FdxeModuleInventory, FdxeModuleError> {
    let mut inventory = FdxeModuleInventory::default();

    for entry in modules {
        let module = resolve_static_module(entry)?;
        let exported_drivers = module.drivers()?;
        inventory.module_count += 1;
        inventory.driver_count += exported_drivers.len();
    }

    Ok(inventory)
}

/// No-alloc runtime registry for discovered FDXE modules and exported drivers.
pub struct FdxeRegistry<'a> {
    modules: &'a mut [MaybeUninit<&'static FdxeModuleV1>],
    drivers: &'a mut [MaybeUninit<&'static FdxeDriverExportV1>],
    module_count: usize,
    driver_count: usize,
    marker: PhantomData<&'a mut ()>,
}

impl<'a> FdxeRegistry<'a> {
    /// Creates one runtime registry over caller-owned storage.
    #[must_use]
    pub fn new(
        modules: &'a mut [MaybeUninit<&'static FdxeModuleV1>],
        drivers: &'a mut [MaybeUninit<&'static FdxeDriverExportV1>],
    ) -> Self {
        Self {
            modules,
            drivers,
            module_count: 0,
            driver_count: 0,
            marker: PhantomData,
        }
    }

    /// Returns the maximum number of module headers this registry can hold.
    #[must_use]
    pub const fn module_capacity(&self) -> usize {
        self.modules.len()
    }

    /// Returns the maximum number of exported driver entries this registry can hold.
    #[must_use]
    pub const fn driver_capacity(&self) -> usize {
        self.drivers.len()
    }

    /// Registers one validated module header and all of its exported driver families.
    ///
    /// # Errors
    ///
    /// Returns an error if the module is invalid, duplicated, or the registry lacks capacity.
    pub fn register_module(
        &mut self,
        module: &'static FdxeModuleV1,
    ) -> Result<(), FdxeModuleError> {
        module.validate()?;

        if self
            .modules()
            .iter()
            .any(|registered| core::ptr::eq(*registered, module))
        {
            return Err(FdxeModuleError::duplicate_module());
        }

        let exported_drivers = module.drivers()?;

        if self.module_count == self.modules.len()
            || self.driver_count + exported_drivers.len() > self.drivers.len()
        {
            return Err(FdxeModuleError::capacity_exhausted());
        }

        self.modules[self.module_count].write(module);
        self.module_count += 1;

        for driver in exported_drivers {
            self.drivers[self.driver_count].write(driver);
            self.driver_count += 1;
        }

        Ok(())
    }

    /// Registers one caller-supplied slice of statically embedded module records.
    ///
    /// # Errors
    ///
    /// Returns an error if any record is malformed or if any embedded module fails normal
    /// registration validation.
    pub fn register_static_modules(
        &mut self,
        modules: &[FdxeStaticModuleV1],
    ) -> Result<(), FdxeModuleError> {
        for entry in modules {
            let module = resolve_static_module(entry)?;
            self.register_module(module)?;
        }

        Ok(())
    }

    /// Returns all registered module headers.
    #[must_use]
    pub fn modules(&self) -> &[&'static FdxeModuleV1] {
        // SAFETY: the prefix `[0..module_count)` is initialized by `register_module`.
        unsafe { slice::from_raw_parts(self.modules.as_ptr().cast(), self.module_count) }
    }

    /// Returns all exported driver entries collected from registered modules.
    #[must_use]
    pub fn drivers(&self) -> &[&'static FdxeDriverExportV1] {
        // SAFETY: the prefix `[0..driver_count)` is initialized by `register_module`.
        unsafe { slice::from_raw_parts(self.drivers.as_ptr().cast(), self.driver_count) }
    }
}

#[cfg(test)]
mod tests {
    use core::mem::MaybeUninit;

    use super::{
        DriverBindingSource,
        DriverClass,
        DriverContractKey,
        DriverIdentity,
        FdxeModuleInventory,
        DriverMetadata,
        DriverUsefulness,
        FDXE_MODULE_V1_ABI_VERSION,
        FDXE_MODULE_V1_MAGIC,
        FdxeDriverExportV1,
        FdxeModuleErrorKind,
        FdxeModuleV1,
        FdxeRegistry,
        FdxeStaticModuleV1,
        inventory_static_modules,
    };

    const CONTRACTS: [DriverContractKey; 1] = [DriverContractKey("test.driver")];
    const REQUIRED_CONTRACTS: [DriverContractKey; 0] = [];
    const BINDINGS: [DriverBindingSource; 1] = [DriverBindingSource::Manual];
    const METADATA: DriverMetadata = DriverMetadata {
        key: "test.driver.example",
        class: DriverClass::Other("test"),
        identity: DriverIdentity {
            vendor: "Fusion",
            family: Some("Tests"),
            package: Some("Spec"),
            product: "Example Driver",
            advertised_interface: "Example",
        },
        contracts: &CONTRACTS,
        required_contracts: &REQUIRED_CONTRACTS,
        usefulness: DriverUsefulness::Standalone,
        singleton_class: None,
        binding_sources: &BINDINGS,
        description: "Example exported driver for fdxe tests",
    };
    const EXPORTS: [FdxeDriverExportV1; 1] =
        [FdxeDriverExportV1::new("test.driver.example", metadata)];
    static MODULE: FdxeModuleV1 = FdxeModuleV1::new("fd-test", "fd-test", &EXPORTS);

    fn metadata() -> &'static DriverMetadata {
        &METADATA
    }

    #[test]
    fn module_header_uses_expected_magic_and_version() {
        assert_eq!(MODULE.magic, FDXE_MODULE_V1_MAGIC);
        assert_eq!(MODULE.abi_version, FDXE_MODULE_V1_ABI_VERSION);
    }

    #[test]
    fn module_registry_collects_exports() {
        let mut module_storage = [MaybeUninit::uninit(); 1];
        let mut driver_storage = [MaybeUninit::uninit(); 1];
        let mut registry = FdxeRegistry::new(&mut module_storage, &mut driver_storage);

        registry
            .register_module(&MODULE)
            .expect("module should register");

        assert_eq!(registry.modules().len(), 1);
        assert_eq!(registry.drivers().len(), 1);
        assert_eq!(
            (registry.drivers()[0].metadata)().key,
            "test.driver.example"
        );
    }

    #[test]
    fn module_registry_rejects_duplicate_module() {
        let mut module_storage = [MaybeUninit::uninit(); 2];
        let mut driver_storage = [MaybeUninit::uninit(); 2];
        let mut registry = FdxeRegistry::new(&mut module_storage, &mut driver_storage);

        registry
            .register_module(&MODULE)
            .expect("first registration should work");
        let error = registry
            .register_module(&MODULE)
            .expect_err("duplicate registration should fail");

        assert_eq!(error.kind(), FdxeModuleErrorKind::DuplicateModule);
    }

    #[test]
    fn module_registry_collects_static_module_records() {
        let mut module_storage = [MaybeUninit::uninit(); 1];
        let mut driver_storage = [MaybeUninit::uninit(); 1];
        let mut registry = FdxeRegistry::new(&mut module_storage, &mut driver_storage);
        let modules = [FdxeStaticModuleV1::new(&MODULE)];

        registry
            .register_static_modules(&modules)
            .expect("static module registration should work");

        assert_eq!(registry.modules().len(), 1);
        assert_eq!(registry.drivers().len(), 1);
    }

    #[test]
    fn inventory_counts_static_modules_and_drivers() {
        let modules = [FdxeStaticModuleV1::new(&MODULE)];
        let inventory = inventory_static_modules(&modules).expect("inventory should succeed");

        assert_eq!(
            inventory,
            FdxeModuleInventory {
                module_count: 1,
                driver_count: 1,
            }
        );
    }
}
