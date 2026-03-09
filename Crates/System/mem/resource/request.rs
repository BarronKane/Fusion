use fusion_pal::sys::mem::{CachePolicy, IntegrityMode, Protect, TagMode};

use super::ops::ResourcePreferenceSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlacementPreference {
    Anywhere,
    Hint(usize),
    PreferredNode(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RequiredPlacement {
    FixedNoReplace(usize),
    RequiredNode(u32),
    RegionId(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InitialResidency {
    BestEffort,
    Prefault,
    Locked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SharingPolicy {
    Private,
    Shared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OvercommitPolicy {
    Allow,
    Disallow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IntegrityConstraints {
    pub mode: IntegrityMode,
    pub tag: Option<TagMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceBackingRequest {
    Anonymous,
    File { fd: i32, offset: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceContract {
    pub allowed_protect: Protect,
    pub write_xor_execute: bool,
    pub sharing: SharingPolicy,
    pub overcommit: OvercommitPolicy,
    pub cache_policy: CachePolicy,
    pub integrity: Option<IntegrityConstraints>,
    pub required_placement: Option<RequiredPlacement>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InitialResourceState {
    pub protect: Protect,
    pub placement: PlacementPreference,
    pub residency: InitialResidency,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceRequest<'a> {
    pub name: Option<&'a str>,
    pub len: usize,
    pub backing: ResourceBackingRequest,
    pub initial: InitialResourceState,
    pub contract: ResourceContract,
    pub preferences: ResourcePreferenceSet,
}

impl<'a> ResourceRequest<'a> {
    #[must_use]
    pub fn anonymous_private(len: usize) -> Self {
        Self {
            name: None,
            len,
            backing: ResourceBackingRequest::Anonymous,
            initial: InitialResourceState {
                protect: Protect::READ | Protect::WRITE,
                placement: PlacementPreference::Anywhere,
                residency: InitialResidency::BestEffort,
            },
            contract: ResourceContract {
                allowed_protect: Protect::READ | Protect::WRITE,
                write_xor_execute: true,
                sharing: SharingPolicy::Private,
                overcommit: OvercommitPolicy::Allow,
                cache_policy: CachePolicy::Default,
                integrity: None,
                required_placement: None,
            },
            preferences: ResourcePreferenceSet::empty(),
        }
    }

    #[must_use]
    pub fn anonymous_shared(len: usize) -> Self {
        let mut request = Self::anonymous_private(len);
        request.contract.sharing = SharingPolicy::Shared;
        request
    }

    #[must_use]
    pub fn file_private(len: usize, fd: i32, offset: u64) -> Self {
        let mut request = Self::anonymous_private(len);
        request.backing = ResourceBackingRequest::File { fd, offset };
        request
    }

    #[must_use]
    pub fn file_shared(len: usize, fd: i32, offset: u64) -> Self {
        let mut request = Self::file_private(len, fd, offset);
        request.contract.sharing = SharingPolicy::Shared;
        request
    }
}
