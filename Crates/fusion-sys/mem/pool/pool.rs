//! Critical-safety-aware memory pooling over compatible realized resources.
//!
//! `MemoryProvider` decides what resources are compatible and how they could be prepared or
//! provisioned. `MemoryPool` owns already-realized compatible contributors and exposes
//! deterministic extent management to the allocator layer above it.
//!
//! The pool deliberately does not pretend multiple resources form one contiguous range. It
//! tracks extents per member resource, returns explicit leases, and keeps all metadata
//! bounded by fixed compile-time capacities. No hidden allocation, no background worker
//! threads, no surprise compaction, and no panic-shaped runtime behavior.

mod builder;
mod error;
mod extent;
mod member;
mod metadata;
mod policy;
mod state;
mod stats;

pub use builder::MemoryPoolBuilder;
pub use error::{MemoryPoolError, MemoryPoolErrorKind};
pub use extent::{
    MemoryPoolExtentRequest, MemoryPoolLease, MemoryPoolLeaseId, MemoryPoolLeaseView,
};
pub use member::{
    MemoryPoolContributor, MemoryPoolContributorOrigin, MemoryPoolMemberId, MemoryPoolMemberInfo,
};
pub use metadata::MemoryPoolMetadataLayout;
pub use policy::{MemoryPoolPolicy, MemoryPoolProvisioningPolicy};
pub use stats::MemoryPoolStats;

use crate::mem::provider::{MemoryCompatibilityEnvelope, MemoryPoolClassId, MemoryTopologyNodeId};
use crate::mem::resource::{MemoryResource, ResourceRange};
use crate::sync::Mutex;

use self::builder::MemoryPoolBuilder as Builder;
use self::extent::{ExtentDisposition, ExtentRecord};
use self::member::MemoryPoolMember;
use self::state::MemoryPoolState;

/// Fixed-capacity pool over compatible realized memory resources.
#[derive(Debug)]
pub struct MemoryPool<const MEMBERS: usize, const EXTENTS: usize> {
    policy: MemoryPoolPolicy,
    compatibility: MemoryCompatibilityEnvelope,
    topology_node: Option<MemoryTopologyNodeId>,
    pool_class: Option<MemoryPoolClassId>,
    members: [Option<MemoryPoolMember>; MEMBERS],
    state: Mutex<MemoryPoolState<EXTENTS>>,
}

impl<const MEMBERS: usize, const EXTENTS: usize> MemoryPool<MEMBERS, EXTENTS> {
    /// Returns the fixed metadata layout of this pool instantiation.
    #[must_use]
    pub const fn metadata_layout() -> MemoryPoolMetadataLayout {
        MemoryPoolMetadataLayout::for_capacities::<MEMBERS, EXTENTS>()
    }

    /// Creates a new builder for this pool shape.
    #[must_use]
    pub fn builder(policy: MemoryPoolPolicy) -> Builder<MEMBERS, EXTENTS> {
        Builder::new(policy)
    }

    pub(super) fn from_builder(
        builder: MemoryPoolBuilder<MEMBERS, EXTENTS>,
    ) -> Result<Self, MemoryPoolError> {
        if builder.contributor_count() == 0 {
            return Err(MemoryPoolError::invalid_request());
        }

        if builder.contributor_count() > EXTENTS {
            return Err(MemoryPoolError::metadata_exhausted());
        }

        let policy = builder.policy();
        let members = builder.into_members()?;
        let (compatibility, topology_node, pool_class) =
            canonical_member_metadata(&members, policy.allow_cross_topology)?;

        let mut state = MemoryPoolState::new();
        for (member_index, member) in members.iter().enumerate() {
            let Some(member) = member else {
                continue;
            };
            let slot = state
                .first_vacant_slot()
                .ok_or_else(MemoryPoolError::metadata_exhausted)?;
            state.extents[slot] = Some(ExtentRecord {
                member_index,
                range: member.usable_range,
                disposition: ExtentDisposition::Free,
            });
            state.free_bytes = state.free_bytes.saturating_add(member.usable_range.len);
        }

        Ok(Self {
            policy,
            compatibility,
            topology_node,
            pool_class,
            members,
            state: Mutex::new(state),
        })
    }

    /// Returns the pool policy.
    #[must_use]
    pub const fn policy(&self) -> MemoryPoolPolicy {
        self.policy
    }

    /// Returns the shared compatibility envelope for all members.
    #[must_use]
    pub const fn compatibility(&self) -> MemoryCompatibilityEnvelope {
        self.compatibility
    }

    /// Returns the canonical topology node for the pool when one is enforced.
    #[must_use]
    pub const fn topology_node(&self) -> Option<MemoryTopologyNodeId> {
        self.topology_node
    }

    /// Returns the canonical pool class when one is enforced.
    #[must_use]
    pub const fn pool_class(&self) -> Option<MemoryPoolClassId> {
        self.pool_class
    }

    /// Returns a pool-wide capacity and extent snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error when the pool cannot synchronize its metadata honestly.
    pub fn stats(&self) -> Result<MemoryPoolStats, MemoryPoolError> {
        let state = self
            .state
            .lock()
            .map_err(|error| MemoryPoolError::synchronization(error.kind))?;
        Ok(state.stats(self.member_count()))
    }

    /// Returns descriptive information about one pool member.
    ///
    /// # Errors
    ///
    /// Returns an error when `member_id` is unknown or the pool cannot synchronize its
    /// metadata honestly.
    pub fn member_info(
        &self,
        member_id: MemoryPoolMemberId,
    ) -> Result<MemoryPoolMemberInfo, MemoryPoolError> {
        let member = self
            .member_by_id(member_id)
            .ok_or_else(MemoryPoolError::invalid_request)?;
        let state = self
            .state
            .lock()
            .map_err(|error| MemoryPoolError::synchronization(error.kind))?;
        let member_index =
            usize::try_from(member_id.0).map_err(|_| MemoryPoolError::invalid_request())?;
        let usage = state.member_usage(member_index);
        Ok(MemoryPoolMemberInfo::from_member(member, usage))
    }

    /// Acquires a new extent lease from the pool.
    ///
    /// # Errors
    ///
    /// Returns an error when the request is invalid, no free extent can satisfy it, or the
    /// pool lacks metadata capacity to represent the resulting split.
    pub fn acquire_extent(
        &self,
        request: &MemoryPoolExtentRequest,
    ) -> Result<MemoryPoolLease, MemoryPoolError> {
        validate_extent_request(request)?;

        let mut state = self
            .state
            .lock()
            .map_err(|error| MemoryPoolError::synchronization(error.kind))?;
        let candidate = self
            .find_candidate(&state, request)
            .ok_or_else(MemoryPoolError::capacity_exhausted)?;
        let lease_id = state.allocate_lease_id()?;

        let free_record =
            state.extents[candidate.slot_index].ok_or_else(MemoryPoolError::capacity_exhausted)?;
        let prefix_len = candidate
            .allocation
            .offset
            .checked_sub(free_record.range.offset)
            .ok_or_else(MemoryPoolError::invalid_request)?;
        let suffix_offset = candidate
            .allocation
            .offset
            .checked_add(candidate.allocation.len)
            .ok_or_else(MemoryPoolError::invalid_request)?;
        let free_end = free_record
            .range
            .offset
            .checked_add(free_record.range.len)
            .ok_or_else(MemoryPoolError::invalid_request)?;
        let suffix_len = free_end
            .checked_sub(suffix_offset)
            .ok_or_else(MemoryPoolError::invalid_request)?;

        let mut needed_slots = 0;
        if prefix_len != 0 {
            needed_slots += 1;
        }
        if suffix_len != 0 {
            needed_slots += 1;
        }
        let extra_slots = state
            .vacant_slots_excluding(needed_slots, Some(candidate.slot_index))
            .ok_or_else(MemoryPoolError::metadata_exhausted)?;

        state.extents[candidate.slot_index] = Some(ExtentRecord {
            member_index: free_record.member_index,
            range: candidate.allocation,
            disposition: ExtentDisposition::Leased(lease_id),
        });

        let mut slot_cursor = 0;
        if prefix_len != 0 {
            let slot = extra_slots[slot_cursor].ok_or_else(MemoryPoolError::metadata_exhausted)?;
            slot_cursor += 1;
            state.extents[slot] = Some(ExtentRecord {
                member_index: free_record.member_index,
                range: ResourceRange::new(free_record.range.offset, prefix_len),
                disposition: ExtentDisposition::Free,
            });
        }
        if suffix_len != 0 {
            let slot = extra_slots[slot_cursor].ok_or_else(MemoryPoolError::metadata_exhausted)?;
            state.extents[slot] = Some(ExtentRecord {
                member_index: free_record.member_index,
                range: ResourceRange::new(suffix_offset, suffix_len),
                disposition: ExtentDisposition::Free,
            });
        }

        state.free_bytes = state.free_bytes.saturating_sub(candidate.allocation.len);
        state.leased_bytes = state.leased_bytes.saturating_add(candidate.allocation.len);

        Ok(MemoryPoolLease {
            id: lease_id,
            member: self.member_id(candidate.member_index)?,
            range: candidate.allocation,
        })
    }

    /// Releases a previously leased extent back to the pool.
    ///
    /// Taking ownership of `lease` is intentional. The lease is a linear capability token,
    /// not a reusable descriptor, and consuming it on release makes use-after-release
    /// mistakes harder to express in higher layers.
    ///
    /// # Errors
    ///
    /// Returns an error when the lease is unknown or the pool cannot synchronize its
    /// metadata honestly.
    #[allow(clippy::needless_pass_by_value)]
    pub fn release_extent(&self, lease: MemoryPoolLease) -> Result<(), MemoryPoolError> {
        let mut state = self
            .state
            .lock()
            .map_err(|error| MemoryPoolError::synchronization(error.kind))?;
        let slot = state
            .extent_slot_for_lease(&lease)
            .ok_or_else(MemoryPoolError::unknown_lease)?;
        let released_len = match state.extents[slot] {
            Some(record) => record.range.len,
            None => return Err(MemoryPoolError::unknown_lease()),
        };
        let Some(record) = &mut state.extents[slot] else {
            return Err(MemoryPoolError::unknown_lease());
        };
        record.disposition = ExtentDisposition::Free;
        state.free_bytes = state.free_bytes.saturating_add(released_len);
        state.leased_bytes = state.leased_bytes.saturating_sub(released_len);
        merge_free_extent_slot(&mut state, slot);
        Ok(())
    }

    /// Returns a borrowed view of a currently leased extent.
    ///
    /// # Errors
    ///
    /// Returns an error when the lease is unknown or the member subrange is no longer
    /// valid for the owned resource.
    pub fn lease_view<'a>(
        &'a self,
        lease: &'a MemoryPoolLease,
    ) -> Result<MemoryPoolLeaseView<'a>, MemoryPoolError> {
        {
            let state = self
                .state
                .lock()
                .map_err(|error| MemoryPoolError::synchronization(error.kind))?;
            if !state.lease_is_active(lease) {
                return Err(MemoryPoolError::unknown_lease());
            }
        }

        let member = self
            .member_by_id(lease.member())
            .ok_or_else(MemoryPoolError::unknown_lease)?;
        let view = member
            .handle
            .subrange(lease.range())
            .map_err(|error| MemoryPoolError::resource(error.kind))?;
        Ok(MemoryPoolLeaseView::new(view))
    }

    fn member_count(&self) -> usize {
        self.members.iter().flatten().count()
    }

    fn member_id(&self, member_index: usize) -> Result<MemoryPoolMemberId, MemoryPoolError> {
        self.members
            .get(member_index)
            .and_then(Option::as_ref)
            .map(|member| member.id)
            .ok_or_else(MemoryPoolError::invalid_request)
    }

    fn member_by_id(&self, member_id: MemoryPoolMemberId) -> Option<&MemoryPoolMember> {
        self.members
            .get(usize::try_from(member_id.0).ok()?)
            .and_then(Option::as_ref)
    }

    fn find_candidate(
        &self,
        state: &MemoryPoolState<EXTENTS>,
        request: &MemoryPoolExtentRequest,
    ) -> Option<AllocationCandidate> {
        let mut best: Option<AllocationCandidate> = None;

        for (slot_index, record) in state.extents.iter().enumerate() {
            let Some(record) = record else {
                continue;
            };
            if !matches!(record.disposition, ExtentDisposition::Free) {
                continue;
            }
            let Some(allocation) =
                self.aligned_allocation(record.member_index, record.range, request)
            else {
                continue;
            };

            let prefix_len = allocation.offset.checked_sub(record.range.offset)?;
            let suffix_offset = allocation.offset.checked_add(allocation.len)?;
            let record_end = record.range.offset.checked_add(record.range.len)?;
            let suffix_len = record_end.checked_sub(suffix_offset)?;
            let metadata_cost = usize::from(prefix_len != 0) + usize::from(suffix_len != 0);
            let waste = record.range.len.saturating_sub(allocation.len);
            let candidate = AllocationCandidate {
                slot_index,
                member_index: record.member_index,
                allocation,
                metadata_cost,
                waste,
            };

            if is_better_candidate(best, candidate) {
                best = Some(candidate);
            }
        }

        best
    }

    fn aligned_allocation(
        &self,
        member_index: usize,
        free_range: ResourceRange,
        request: &MemoryPoolExtentRequest,
    ) -> Option<ResourceRange> {
        let member = self.members.get(member_index)?.as_ref()?;
        let base = {
            let view = member.handle.view();
            view.base_addr().get()
        };
        let start_addr = base.checked_add(free_range.offset)?;
        let aligned_start = align_up(start_addr, request.align)?;
        let prefix = aligned_start.checked_sub(start_addr)?;
        if prefix > free_range.len {
            return None;
        }

        let offset = free_range.offset.checked_add(prefix)?;
        let available = free_range.len.checked_sub(prefix)?;
        if request.len > available {
            return None;
        }

        Some(ResourceRange::new(offset, request.len))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct AllocationCandidate {
    slot_index: usize,
    member_index: usize,
    allocation: ResourceRange,
    metadata_cost: usize,
    waste: usize,
}

fn is_better_candidate(
    current: Option<AllocationCandidate>,
    candidate: AllocationCandidate,
) -> bool {
    current.is_none_or(|current| {
        (
            candidate.metadata_cost,
            candidate.waste,
            candidate.member_index,
            candidate.slot_index,
        ) < (
            current.metadata_cost,
            current.waste,
            current.member_index,
            current.slot_index,
        )
    })
}

fn merge_free_extent_slot<const EXTENTS: usize>(
    state: &mut MemoryPoolState<EXTENTS>,
    anchor_slot: usize,
) {
    loop {
        let Some(anchor) = state.extents[anchor_slot] else {
            return;
        };
        let mut merged = false;

        for index in 0..EXTENTS {
            if index == anchor_slot {
                continue;
            }
            let Some(candidate) = state.extents[index] else {
                continue;
            };
            if candidate.member_index != anchor.member_index
                || !matches!(candidate.disposition, ExtentDisposition::Free)
            {
                continue;
            }

            let anchor_start = anchor.range.offset;
            let Some(anchor_end) = anchor.range.offset.checked_add(anchor.range.len) else {
                return;
            };
            let candidate_start = candidate.range.offset;
            let Some(candidate_end) = candidate.range.offset.checked_add(candidate.range.len)
            else {
                return;
            };

            let merged_range = if candidate_end == anchor_start {
                Some(ResourceRange::new(
                    candidate_start,
                    match candidate.range.len.checked_add(anchor.range.len) {
                        Some(value) => value,
                        None => return,
                    },
                ))
            } else if anchor_end == candidate_start {
                Some(ResourceRange::new(
                    anchor_start,
                    match anchor.range.len.checked_add(candidate.range.len) {
                        Some(value) => value,
                        None => return,
                    },
                ))
            } else {
                None
            };

            if let Some(range) = merged_range {
                state.extents[anchor_slot] = Some(ExtentRecord { range, ..anchor });
                state.extents[index] = None;
                merged = true;
                break;
            }
        }

        if !merged {
            break;
        }
    }
}

fn canonical_member_metadata<const MEMBERS: usize>(
    members: &[Option<MemoryPoolMember>; MEMBERS],
    allow_cross_topology: bool,
) -> Result<
    (
        MemoryCompatibilityEnvelope,
        Option<MemoryTopologyNodeId>,
        Option<MemoryPoolClassId>,
    ),
    MemoryPoolError,
> {
    let first = members
        .iter()
        .flatten()
        .next()
        .ok_or_else(MemoryPoolError::invalid_request)?;
    let compatibility = first.compatibility;
    let topology_node = first.topology_node;
    let pool_class = first.pool_class;

    for member in members.iter().flatten().skip(1) {
        if member.compatibility != compatibility {
            return Err(MemoryPoolError::incompatible_contributor());
        }
        if member.pool_class != pool_class {
            return Err(MemoryPoolError::incompatible_contributor());
        }
        if !allow_cross_topology && member.topology_node != topology_node {
            return Err(MemoryPoolError::incompatible_contributor());
        }
    }

    Ok((compatibility, topology_node, pool_class))
}

const fn validate_extent_request(request: &MemoryPoolExtentRequest) -> Result<(), MemoryPoolError> {
    if request.len == 0 || request.align == 0 || !request.align.is_power_of_two() {
        return Err(MemoryPoolError::invalid_request());
    }

    Ok(())
}

fn align_up(value: usize, align: usize) -> Option<usize> {
    let mask = align.checked_sub(1)?;
    value.checked_add(mask).map(|value| value & !mask)
}
