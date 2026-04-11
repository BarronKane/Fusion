//! PCI virtualization vocabulary.
//!
//! This lane intentionally keeps SR-IOV truth sparse until a real backend needs more. In
//! particular, `PciSriovProfile` does not derive `Default`: the optional VF device id is a real
//! piece of typed truth, and inventing a zero/default device id would be a lie dressed up as
//! ergonomics.

use super::core::*;

/// SR-IOV capability truth for one PF.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PciSriovProfile {
    pub total_vfs: u16,
    pub initial_vfs: u16,
    pub enabled_vfs: u16,
    pub vf_stride: u16,
    pub vf_device_id: Option<PciDeviceId>,
}

/// Virtualization capability truth for one function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PciVirtualizationProfile {
    pub ari: bool,
    pub sr_iov: Option<PciSriovProfile>,
}

/// Virtualization lane for one PCI function.
pub trait PciVirtualizationContract {
    /// Returns one truthful virtualization capability snapshot.
    fn virtualization_profile(&self) -> PciVirtualizationProfile;
}
