//! Runtime-loadable Fusion driver module ABI.

#[cfg(target_os = "none")]
use core::cell::UnsafeCell;
#[cfg(target_os = "none")]
use core::hint::spin_loop;
#[cfg(target_os = "none")]
use core::sync::atomic::{
    AtomicU8,
    Ordering,
};

use fusion_hal::contract::drivers::driver::{
    DriverError,
    DriverMetadata,
};
use fusion_hal::contract::drivers::net::bluetooth::BluetoothAdapterContract;
use fusion_hal::contract::drivers::net::wifi::WifiAdapterContract;
#[cfg(test)]
use fusion_hal::contract::drivers::driver::{
    DriverBindingSource,
    DriverClass,
    DriverContractKey,
    DriverIdentity,
    DriverUsefulness,
};

include!(concat!(env!("OUT_DIR"), "/fdxe_shared.rs"));
include!(concat!(env!("OUT_DIR"), "/selected_fdxe_requests.rs"));

#[cfg(target_os = "none")]
const REQUESTED_FDXE_REGISTRY_UNINITIALIZED: u8 = 0;
#[cfg(target_os = "none")]
const REQUESTED_FDXE_REGISTRY_RUNNING: u8 = 1;
#[cfg(target_os = "none")]
const REQUESTED_FDXE_REGISTRY_READY: u8 = 2;

#[cfg(target_os = "none")]
unsafe extern "C" {
    static __fusion_fdxe_modules_start: u8;
    static __fusion_fdxe_modules_end: u8;
}

#[cfg(target_os = "none")]
struct RequestedFdxeRegistryCache {
    state: AtomicU8,
    modules:
        UnsafeCell<[core::mem::MaybeUninit<&'static FdxeModuleV1>; REQUESTED_FDXE_MODULE_CAPACITY]>,
    drivers: UnsafeCell<
        [core::mem::MaybeUninit<&'static FdxeDriverExportV1>; REQUESTED_FDXE_DRIVER_CAPACITY],
    >,
    registry: UnsafeCell<core::mem::MaybeUninit<FdxeRegistry<'static>>>,
}

#[cfg(target_os = "none")]
impl RequestedFdxeRegistryCache {
    const fn new() -> Self {
        Self {
            state: AtomicU8::new(REQUESTED_FDXE_REGISTRY_UNINITIALIZED),
            modules: UnsafeCell::new(
                [const { core::mem::MaybeUninit::uninit() }; REQUESTED_FDXE_MODULE_CAPACITY],
            ),
            drivers: UnsafeCell::new(
                [const { core::mem::MaybeUninit::uninit() }; REQUESTED_FDXE_DRIVER_CAPACITY],
            ),
            registry: UnsafeCell::new(core::mem::MaybeUninit::uninit()),
        }
    }

    fn get_or_init(&self) -> Result<&'static FdxeRegistry<'static>, DriverError> {
        loop {
            match self.state.load(Ordering::Acquire) {
                REQUESTED_FDXE_REGISTRY_READY => {
                    // SAFETY: READY only becomes visible after the registry has been fully
                    // initialized and published for process-lifetime shared reads.
                    let registry = unsafe { &*(*self.registry.get()).as_ptr() };
                    return Ok(registry);
                }
                REQUESTED_FDXE_REGISTRY_UNINITIALIZED => {
                    if self
                        .state
                        .compare_exchange(
                            REQUESTED_FDXE_REGISTRY_UNINITIALIZED,
                            REQUESTED_FDXE_REGISTRY_RUNNING,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_err()
                    {
                        continue;
                    }

                    let modules = unsafe { &mut *self.modules.get() };
                    let drivers = unsafe { &mut *self.drivers.get() };
                    let inventory = match inventory_requested_static_modules() {
                        Ok(inventory) => inventory,
                        Err(error) => {
                            self.state
                                .store(REQUESTED_FDXE_REGISTRY_UNINITIALIZED, Ordering::Release);
                            return Err(error.into());
                        }
                    };
                    if inventory.module_count > modules.len()
                        || inventory.driver_count > drivers.len()
                    {
                        self.state
                            .store(REQUESTED_FDXE_REGISTRY_UNINITIALIZED, Ordering::Release);
                        return Err(DriverError::resource_exhausted());
                    }
                    let mut registry = FdxeRegistry::new(modules, drivers);

                    match register_requested_static_modules(&mut registry) {
                        Ok(()) => {
                            // SAFETY: the registry is written exactly once before READY is
                            // published and points only at static storage owned by this cache.
                            unsafe { (*self.registry.get()).write(registry) };
                            self.state
                                .store(REQUESTED_FDXE_REGISTRY_READY, Ordering::Release);
                            // SAFETY: the registry write completed before READY was published.
                            let registry = unsafe { &*(*self.registry.get()).as_ptr() };
                            return Ok(registry);
                        }
                        Err(error) => {
                            self.state
                                .store(REQUESTED_FDXE_REGISTRY_UNINITIALIZED, Ordering::Release);
                            return Err(error.into());
                        }
                    }
                }
                REQUESTED_FDXE_REGISTRY_RUNNING => spin_loop(),
                _ => return Err(DriverError::state_conflict()),
            }
        }
    }
}

#[cfg(target_os = "none")]
// SAFETY: all shared mutable state is serialized by the one-time state machine above.
unsafe impl Sync for RequestedFdxeRegistryCache {}

#[cfg(target_os = "none")]
static REQUESTED_FDXE_REGISTRY: RequestedFdxeRegistryCache = RequestedFdxeRegistryCache::new();

/// Registers all statically embedded driver modules from the firmware image.
///
/// # Errors
///
/// Returns an error if the linker section layout is invalid or any embedded module fails normal
/// FDXE validation.
#[cfg(target_os = "none")]
pub fn register_static_modules(registry: &mut FdxeRegistry<'_>) -> Result<(), FdxeModuleError> {
    let modules = static_modules()?;
    let inventory = inventory_static_modules(modules)?;
    if inventory.module_count > registry.module_capacity()
        || inventory.driver_count > registry.driver_capacity()
    {
        return Err(FdxeModuleError::capacity_exhausted());
    }
    registry.register_static_modules(modules)
}

/// Registers only the currently requested statically embedded driver modules.
///
/// # Errors
///
/// Returns an error if the linker section layout is invalid or any selected module fails FDXE
/// validation.
#[cfg(target_os = "none")]
pub fn register_requested_static_modules(
    registry: &mut FdxeRegistry<'_>,
) -> Result<(), FdxeModuleError> {
    let modules = static_modules()?;
    let requested = requested_module_crate_names();
    let inventory = inventory_requested_static_modules()?;
    if inventory.module_count > registry.module_capacity()
        || inventory.driver_count > registry.driver_capacity()
    {
        return Err(FdxeModuleError::capacity_exhausted());
    }

    for entry in modules {
        let module = resolve_static_module(entry)?;
        let module_name = module.module_name()?;
        if requested.is_empty() || requested.iter().any(|requested| *requested == module_name) {
            registry.register_module(module)?;
        }
    }

    Ok(())
}

/// Inventories the statically embedded module set selected for this firmware image.
///
/// # Errors
///
/// Returns an error if the linker section layout is invalid or any selected module fails normal
/// FDXE validation.
#[cfg(target_os = "none")]
pub fn inventory_requested_static_modules() -> Result<FdxeModuleInventory, FdxeModuleError> {
    let modules = static_modules()?;
    let requested = requested_module_crate_names();
    let mut inventory = FdxeModuleInventory::default();

    for entry in modules {
        let module = resolve_static_module(entry)?;
        let module_name = module.module_name()?;
        if requested.is_empty() || requested.iter().any(|requested| *requested == module_name) {
            let exported = module.drivers()?;
            inventory.module_count += 1;
            inventory.driver_count += exported.len();
        }
    }

    Ok(inventory)
}

/// Returns the build-selected FDXE module crate names requested for this firmware image.
#[must_use]
pub const fn requested_module_crate_names() -> &'static [&'static str] {
    REQUESTED_FDXE_MODULE_CRATE_NAMES
}

/// Returns the statically available FDXE module crate names enabled in this firmware build.
#[must_use]
pub const fn available_module_crate_names() -> &'static [&'static str] {
    AVAILABLE_FDXE_MODULE_CRATE_NAMES
}

/// Returns the fixed-capacity module-slot count required for all requested static FDXE modules.
#[must_use]
pub const fn requested_module_capacity() -> usize {
    REQUESTED_FDXE_MODULE_CAPACITY
}

/// Returns the fixed-capacity driver-slot count required for all requested static FDXE drivers.
#[must_use]
pub const fn requested_driver_capacity() -> usize {
    REQUESTED_FDXE_DRIVER_CAPACITY
}

/// Returns one registered module by its stable crate/module name.
#[must_use]
pub fn module_by_name<'a>(
    registry: &'a FdxeRegistry<'_>,
    module_name: &str,
) -> Option<&'a FdxeModuleV1> {
    registry
        .modules()
        .iter()
        .copied()
        .find(|module| module.module_name().ok() == Some(module_name))
}

/// Returns one registered driver export by its canonical driver key.
#[must_use]
pub fn driver_by_key(
    registry: &FdxeRegistry<'_>,
    key: &str,
) -> Option<&'static FdxeDriverExportV1> {
    registry
        .drivers()
        .iter()
        .copied()
        .find(|driver| driver.driver_key() == key)
}

/// Fixed-capacity caller-owned storage for one requested static FDXE registry walk.
#[cfg(target_os = "none")]
pub struct RequestedFdxeRegistryStorage {
    modules: [core::mem::MaybeUninit<&'static FdxeModuleV1>; REQUESTED_FDXE_MODULE_CAPACITY],
    drivers: [core::mem::MaybeUninit<&'static FdxeDriverExportV1>; REQUESTED_FDXE_DRIVER_CAPACITY],
}

#[cfg(target_os = "none")]
impl RequestedFdxeRegistryStorage {
    /// Creates one empty caller-owned storage block sized exactly for the requested static FDXE
    /// module set selected at build time.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            modules: [const { core::mem::MaybeUninit::uninit() }; REQUESTED_FDXE_MODULE_CAPACITY],
            drivers: [const { core::mem::MaybeUninit::uninit() }; REQUESTED_FDXE_DRIVER_CAPACITY],
        }
    }

    /// Builds one selected static-module registry in caller-owned storage.
    ///
    /// # Errors
    ///
    /// Returns one truthful driver error when module validation fails or the generated capacities
    /// are no longer honest for the selected firmware image.
    pub fn build_registry(&mut self) -> Result<FdxeRegistry<'_>, DriverError> {
        let inventory = inventory_requested_static_modules()?;
        if inventory.module_count > self.modules.len()
            || inventory.driver_count > self.drivers.len()
        {
            return Err(DriverError::resource_exhausted());
        }
        let mut registry = FdxeRegistry::new(&mut self.modules, &mut self.drivers);
        register_requested_static_modules(&mut registry)?;
        Ok(registry)
    }
}

#[cfg(target_os = "none")]
impl Default for RequestedFdxeRegistryStorage {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns one requested static driver export by its canonical driver key.
///
/// # Errors
///
/// Returns one truthful driver error when selected static modules cannot be registered honestly or
/// when the requested driver family is absent from the selected firmware image.
#[cfg(target_os = "none")]
pub fn requested_driver_by_key(key: &str) -> Result<&'static FdxeDriverExportV1, DriverError> {
    let registry = REQUESTED_FDXE_REGISTRY.get_or_init()?;
    driver_by_key(registry, key).ok_or_else(DriverError::unsupported)
}

/// Returns the linker-collected slice of statically embedded module records.
///
/// # Errors
///
/// Returns an error if the linker section bounds are not aligned to the embedded-record layout.
#[cfg(target_os = "none")]
pub fn static_modules() -> Result<&'static [FdxeStaticModuleV1], FdxeModuleError> {
    let start = core::ptr::addr_of!(__fusion_fdxe_modules_start) as usize;
    let end = core::ptr::addr_of!(__fusion_fdxe_modules_end) as usize;

    if end < start {
        return Err(FdxeModuleError::layout_mismatch());
    }

    let record_size = core::mem::size_of::<FdxeStaticModuleV1>();
    if record_size == 0 {
        return Err(FdxeModuleError::layout_mismatch());
    }

    let byte_len = end - start;
    if byte_len % record_size != 0 {
        return Err(FdxeModuleError::layout_mismatch());
    }

    let align = core::mem::align_of::<FdxeStaticModuleV1>();
    if start % align != 0 {
        return Err(FdxeModuleError::layout_mismatch());
    }

    let count = byte_len / record_size;
    // SAFETY: the linker script defines a contiguous section of `FdxeStaticModuleV1` records.
    let modules = unsafe { core::slice::from_raw_parts(start as *const FdxeStaticModuleV1, count) };
    Ok(modules)
}

/// Stack-owned raw storage used to bind one selected driver without allocation.
pub struct StackDriverStorage<const WORDS: usize> {
    words: [core::mem::MaybeUninit<usize>; WORDS],
}

impl<const WORDS: usize> StackDriverStorage<WORDS> {
    /// Creates one empty stack driver-storage block.
    #[must_use]
    pub fn new() -> Self {
        Self {
            words: [const { core::mem::MaybeUninit::uninit() }; WORDS],
        }
    }

    /// Returns this storage as one one-shot placement slot.
    #[must_use]
    pub fn slot(&mut self) -> StackDriverSlot<'_> {
        StackDriverSlot {
            ptr: self.words.as_mut_ptr().cast(),
            len_bytes: core::mem::size_of::<[core::mem::MaybeUninit<usize>; WORDS]>(),
            align_bytes: core::mem::align_of::<[core::mem::MaybeUninit<usize>; WORDS]>(),
            marker: core::marker::PhantomData,
        }
    }
}

impl<const WORDS: usize> Default for StackDriverStorage<WORDS> {
    fn default() -> Self {
        Self::new()
    }
}

/// One stack-owned placement slot used to bind one selected driver instance.
pub struct StackDriverSlot<'a> {
    ptr: *mut u8,
    len_bytes: usize,
    align_bytes: usize,
    marker: core::marker::PhantomData<&'a mut [u8]>,
}

impl<'a> StackDriverSlot<'a> {
    fn place<T>(self, value: T) -> Result<*mut T, DriverError> {
        if core::mem::size_of::<T>() > self.len_bytes
            || core::mem::align_of::<T>() > self.align_bytes
        {
            return Err(DriverError::resource_exhausted());
        }

        let ptr = self.ptr.cast::<T>();
        if (ptr as usize) % core::mem::align_of::<T>() != 0 {
            return Err(DriverError::invalid());
        }

        // SAFETY: the caller proved the slot is large and aligned enough for `T`.
        unsafe { ptr.write(value) };
        Ok(ptr)
    }
}

/// One stack-bound opened Bluetooth adapter.
pub struct StackBluetoothAdapter<'a> {
    metadata: &'static DriverMetadata,
    instance: *mut (),
    drop_in_place: unsafe fn(*mut ()),
    as_adapter: unsafe fn(*mut ()) -> *mut dyn BluetoothAdapterContract,
    marker: core::marker::PhantomData<&'a mut ()>,
}

impl<'a> StackBluetoothAdapter<'a> {
    /// Returns the truthful metadata for the selected driver family that created this adapter.
    #[must_use]
    pub const fn metadata(&self) -> &'static DriverMetadata {
        self.metadata
    }

    /// Returns this stack-bound adapter as the canonical public Bluetooth adapter contract.
    #[must_use]
    pub fn adapter(&self) -> &dyn BluetoothAdapterContract {
        // SAFETY: the binding helper only installs `as_adapter` for matching concrete types.
        unsafe { &*(self.as_adapter)(self.instance) }
    }

    /// Returns this stack-bound adapter as the canonical mutable public Bluetooth adapter
    /// contract.
    #[must_use]
    pub fn adapter_mut(&mut self) -> &mut dyn BluetoothAdapterContract {
        // SAFETY: the binding helper only installs `as_adapter` for matching concrete types.
        unsafe { &mut *(self.as_adapter)(self.instance) }
    }
}

impl Drop for StackBluetoothAdapter<'_> {
    fn drop(&mut self) {
        // SAFETY: `instance` points at the live concrete adapter placed into caller-owned storage.
        unsafe { (self.drop_in_place)(self.instance) };
    }
}

/// One stack-bound opened Wi-Fi adapter.
pub struct StackWifiAdapter<'a> {
    metadata: &'static DriverMetadata,
    instance: *mut (),
    drop_in_place: unsafe fn(*mut ()),
    as_adapter: unsafe fn(*mut ()) -> *mut dyn WifiAdapterContract,
    marker: core::marker::PhantomData<&'a mut ()>,
}

impl<'a> StackWifiAdapter<'a> {
    /// Returns the truthful metadata for the selected driver family that created this adapter.
    #[must_use]
    pub const fn metadata(&self) -> &'static DriverMetadata {
        self.metadata
    }

    /// Returns this stack-bound adapter as the canonical public Wi-Fi adapter contract.
    #[must_use]
    pub fn adapter(&self) -> &dyn WifiAdapterContract {
        // SAFETY: the binding helper only installs `as_adapter` for matching concrete types.
        unsafe { &*(self.as_adapter)(self.instance) }
    }

    /// Returns this stack-bound adapter as the canonical mutable public Wi-Fi adapter contract.
    #[must_use]
    pub fn adapter_mut(&mut self) -> &mut dyn WifiAdapterContract {
        // SAFETY: the binding helper only installs `as_adapter` for matching concrete types.
        unsafe { &mut *(self.as_adapter)(self.instance) }
    }
}

impl Drop for StackWifiAdapter<'_> {
    fn drop(&mut self) {
        // SAFETY: `instance` points at the live concrete adapter placed into caller-owned storage.
        unsafe { (self.drop_in_place)(self.instance) };
    }
}

unsafe fn drop_in_place<T>(instance: *mut ()) {
    unsafe { instance.cast::<T>().drop_in_place() };
}

unsafe fn as_bluetooth_adapter<T: BluetoothAdapterContract>(
    instance: *mut (),
) -> *mut dyn BluetoothAdapterContract {
    instance.cast::<T>() as *mut dyn BluetoothAdapterContract
}

unsafe fn as_wifi_adapter<T: WifiAdapterContract>(
    instance: *mut (),
) -> *mut dyn WifiAdapterContract {
    instance.cast::<T>() as *mut dyn WifiAdapterContract
}

/// Places one concrete Bluetooth adapter into caller-owned stack storage and returns the bound
/// public contract wrapper.
pub fn bind_bluetooth_adapter<'a, T>(
    slot: StackDriverSlot<'a>,
    metadata: &'static DriverMetadata,
    adapter: T,
) -> Result<StackBluetoothAdapter<'a>, DriverError>
where
    T: BluetoothAdapterContract + 'static,
{
    let instance = slot.place(adapter)?;
    Ok(StackBluetoothAdapter {
        metadata,
        instance: instance.cast(),
        drop_in_place: drop_in_place::<T>,
        as_adapter: as_bluetooth_adapter::<T>,
        marker: core::marker::PhantomData,
    })
}

/// Places one concrete Wi-Fi adapter into caller-owned stack storage and returns the bound public
/// contract wrapper.
pub fn bind_wifi_adapter<'a, T>(
    slot: StackDriverSlot<'a>,
    metadata: &'static DriverMetadata,
    adapter: T,
) -> Result<StackWifiAdapter<'a>, DriverError>
where
    T: WifiAdapterContract + 'static,
{
    let instance = slot.place(adapter)?;
    Ok(StackWifiAdapter {
        metadata,
        instance: instance.cast(),
        drop_in_place: drop_in_place::<T>,
        as_adapter: as_wifi_adapter::<T>,
        marker: core::marker::PhantomData,
    })
}
