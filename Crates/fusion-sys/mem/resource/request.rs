use fusion_pal::sys::mem::{
    BorrowedBackingHandle,
    CachePolicy,
    IntegrityMode,
    Protect,
    TagMode,
};

use super::ops::ResourcePreferenceSet;

/// Soft placement policy used when creating a resource or reservation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlacementPreference {
    /// Allow the backend to choose any suitable address.
    Anywhere,
    /// Suggest an address without requiring it.
    Hint(usize),
    /// Prefer a NUMA node when the backend can honor it.
    PreferredNode(u32),
}

/// Hard placement requirement that must be satisfied or creation fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RequiredPlacement {
    /// Require an exact address and fail rather than replace another mapping.
    FixedNoReplace(usize),
    /// Require placement on a specific NUMA node.
    RequiredNode(u32),
    /// Require placement within a backend-defined region identifier.
    RegionId(u64),
}

/// Requested initial residency policy for a newly created resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InitialResidency {
    /// Accept ordinary lazy faulting behavior.
    BestEffort,
    /// Request eager population of backing pages when the backend exposes that path.
    ///
    /// This improves acquisition intent but does not, by itself, prove that every page is
    /// resident after creation on all platforms.
    Prefault,
    /// Require a verified lock or pin step at creation time.
    ///
    /// Creation fails if the backend cannot establish the requested locked residency.
    Locked,
}

/// Sharing contract for the created resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SharingPolicy {
    /// Resource contents are private to the creating address space.
    Private,
    /// Resource contents may be visible through shared aliases.
    Shared,
}

/// Overcommit policy requested from the backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OvercommitPolicy {
    /// Allow the backend's normal lazy commitment or overcommit behavior.
    Allow,
    /// Require stronger no-overcommit semantics when available.
    Disallow,
}

/// Integrity and tag-mode constraints requested for a resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IntegrityConstraints {
    /// Requested integrity mode for the backing.
    pub mode: IntegrityMode,
    /// Optional tag-mode policy layered on top of the integrity regime.
    pub tag: Option<TagMode>,
}

/// Backing object requested when creating a virtual memory resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceBackingRequest<'a> {
    /// Fresh anonymous backing with no external object.
    Anonymous,
    /// File-backed memory using the supplied borrowed handle and byte offset.
    File {
        /// Borrowed platform handle naming the backing object.
        fd: BorrowedBackingHandle<'a>,
        /// Byte offset into the file-backed object.
        offset: u64,
    },
}

/// Immutable lifetime rules that a created resource must continue to satisfy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceContract {
    /// Maximum protection set the resource may ever hold.
    pub allowed_protect: Protect,
    /// Whether writable and executable protections must remain mutually exclusive.
    pub write_xor_execute: bool,
    /// Sharing behavior that the created resource must preserve.
    pub sharing: SharingPolicy,
    /// Overcommit policy requested from the backend.
    pub overcommit: OvercommitPolicy,
    /// Cache policy requested for the range.
    pub cache_policy: CachePolicy,
    /// Optional integrity/tag-mode constraints.
    pub integrity: Option<IntegrityConstraints>,
}

/// Requested initial runtime state for a newly created resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InitialResourceState {
    /// Initial protection applied to the created range.
    pub protect: Protect,
    /// Initial soft placement preference.
    pub placement: PlacementPreference,
    /// Initial residency policy.
    pub residency: InitialResidency,
}

/// Complete request for creating a virtual memory resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceRequest<'a> {
    /// Optional human-readable name for diagnostics or provider-specific bookkeeping.
    pub name: Option<&'a str>,
    /// Requested length in bytes before backend rounding.
    pub len: usize,
    /// Requested backing object or backing class.
    pub backing: ResourceBackingRequest<'a>,
    /// Requested initial runtime state.
    pub initial: InitialResourceState,
    /// Hard placement requirement that must be satisfied or creation fails.
    pub required_placement: Option<RequiredPlacement>,
    /// Immutable lifetime contract for the created resource.
    pub contract: ResourceContract,
    /// Soft preferences the backend may try to honor.
    pub preferences: ResourcePreferenceSet,
}

impl<'a> ResourceRequest<'a> {
    /// Returns a default anonymous private resource request for `len` bytes.
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
            required_placement: None,
            contract: ResourceContract {
                allowed_protect: Protect::READ | Protect::WRITE,
                write_xor_execute: true,
                sharing: SharingPolicy::Private,
                overcommit: OvercommitPolicy::Allow,
                cache_policy: CachePolicy::Default,
                integrity: None,
            },
            preferences: ResourcePreferenceSet::empty(),
        }
    }

    /// Returns a default anonymous shared resource request for `len` bytes.
    #[must_use]
    pub fn anonymous_shared(len: usize) -> Self {
        let mut request = Self::anonymous_private(len);
        request.contract.sharing = SharingPolicy::Shared;
        request
    }

    /// Returns a default privately mapped file-backed resource request.
    #[must_use]
    pub fn file_private(len: usize, fd: BorrowedBackingHandle<'a>, offset: u64) -> Self {
        let mut request = Self::anonymous_private(len);
        request.backing = ResourceBackingRequest::File { fd, offset };
        request
    }

    /// Returns a default shared file-backed resource request.
    #[must_use]
    pub fn file_shared(len: usize, fd: BorrowedBackingHandle<'a>, offset: u64) -> Self {
        let mut request = Self::file_private(len, fd, offset);
        request.contract.sharing = SharingPolicy::Shared;
        request
    }
}
