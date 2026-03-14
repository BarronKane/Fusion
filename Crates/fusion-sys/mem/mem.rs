#[path = "resource/resource.rs"]
/// Contiguous governed memory resources and reservations.
pub mod resource;

#[path = "provider/provider.rs"]
/// Resource orchestration, topology, and pool-facing provider contracts.
pub mod provider;

#[path = "pool/pool.rs"]
/// Critical-safety-aware pooling over compatible realized memory resources.
pub mod pool;
