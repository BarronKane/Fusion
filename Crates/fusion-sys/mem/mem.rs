#[path = "resource/resource.rs"]
/// Contiguous governed memory resources and reservations.
pub mod resource;

#[path = "provider/provider.rs"]
/// Resource orchestration, topology, and pool-facing provider contracts.
pub mod provider;

#[path = "pool/pool.rs"]
/// Internal pool substrate consumed by `fusion-sys::alloc`.
pub(crate) mod pool;
