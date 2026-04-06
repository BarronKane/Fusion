//! DriverContract-facing Wi-Fi contract vocabulary.

mod ap;
mod base;
mod caps;
mod data;
mod error;
mod mesh;
mod monitor;
mod p2p;
mod radio;
mod scan;
mod security;
mod spec;
mod station;
mod types;
mod unsupported;

pub use ap::*;
pub use base::*;
pub use caps::*;
pub use data::*;
pub use error::*;
pub use mesh::*;
pub use monitor::*;
pub use p2p::*;
pub use radio::*;
pub use scan::*;
pub use security::*;
pub use spec::*;
pub use station::*;
pub use types::*;
pub use unsupported::*;

/// Full control surface for one opened Wi-Fi adapter.
pub trait WifiAdapterContract:
    WifiOwnedAdapterContract
    + WifiRadioControlContract
    + WifiScanControlContract
    + WifiStationControlContract
    + WifiSecurityControlContract
    + WifiAccessPointControlContract
    + WifiDataControlContract
    + WifiMonitorControlContract
    + WifiP2pControlContract
    + WifiMeshControlContract
{
}

impl<T> WifiAdapterContract for T where
    T: WifiOwnedAdapterContract
        + WifiRadioControlContract
        + WifiScanControlContract
        + WifiStationControlContract
        + WifiSecurityControlContract
        + WifiAccessPointControlContract
        + WifiDataControlContract
        + WifiMonitorControlContract
        + WifiP2pControlContract
        + WifiMeshControlContract
{
}
