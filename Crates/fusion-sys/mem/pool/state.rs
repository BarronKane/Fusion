use core::array;

use super::extent::{ExtentDisposition, ExtentRecord, MemoryPoolLease, MemoryPoolLeaseId};
use super::member::MemberUsageStats;
use super::{MemoryPoolError, MemoryPoolStats};

#[derive(Debug)]
pub(super) struct MemoryPoolState<const EXTENTS: usize> {
    pub extents: [Option<ExtentRecord>; EXTENTS],
    pub free_bytes: usize,
    pub leased_bytes: usize,
    pub next_lease_id: u64,
}

impl<const EXTENTS: usize> MemoryPoolState<EXTENTS> {
    pub(super) fn new() -> Self {
        Self {
            extents: array::from_fn(|_| None),
            free_bytes: 0,
            leased_bytes: 0,
            next_lease_id: 1,
        }
    }

    pub(super) fn allocate_lease_id(&mut self) -> Result<MemoryPoolLeaseId, MemoryPoolError> {
        if self.next_lease_id == 0 {
            return Err(MemoryPoolError::metadata_exhausted());
        }

        let id = MemoryPoolLeaseId(self.next_lease_id);
        self.next_lease_id = self.next_lease_id.checked_add(1).unwrap_or(0);
        Ok(id)
    }

    pub(super) fn first_vacant_slot(&self) -> Option<usize> {
        self.extents.iter().position(Option::is_none)
    }

    pub(super) fn vacant_slots_excluding(
        &self,
        needed: usize,
        excluded: Option<usize>,
    ) -> Option<[Option<usize>; 2]> {
        let mut slots = [None, None];
        if needed == 0 {
            return Some(slots);
        }

        let mut found = 0;
        for (index, slot) in self.extents.iter().enumerate() {
            if excluded == Some(index) {
                continue;
            }
            if slot.is_none() {
                slots[found] = Some(index);
                found += 1;
                if found == needed {
                    return Some(slots);
                }
            }
        }

        None
    }

    pub(super) fn extent_slot_for_lease(&self, lease: &MemoryPoolLease) -> Option<usize> {
        self.extents.iter().position(|record| {
            matches!(
                record,
                Some(ExtentRecord {
                    member_index,
                    range,
                    disposition: ExtentDisposition::Leased(id),
                }) if usize::try_from(lease.member().0).ok() == Some(*member_index)
                    && *range == lease.range()
                    && *id == lease.id()
            )
        })
    }

    pub(super) fn lease_is_active(&self, lease: &MemoryPoolLease) -> bool {
        self.extent_slot_for_lease(lease).is_some()
    }

    pub(super) fn member_usage(&self, member_index: usize) -> MemberUsageStats {
        let mut free_bytes: usize = 0;
        let mut leased_bytes: usize = 0;
        let mut largest_free_extent: usize = 0;

        for record in self.extents.iter().flatten() {
            if record.member_index != member_index {
                continue;
            }

            match record.disposition {
                ExtentDisposition::Free => {
                    free_bytes = free_bytes.saturating_add(record.range.len);
                    largest_free_extent = largest_free_extent.max(record.range.len);
                }
                ExtentDisposition::Leased(_) => {
                    leased_bytes = leased_bytes.saturating_add(record.range.len);
                }
            }
        }

        MemberUsageStats::new(free_bytes, leased_bytes, largest_free_extent)
    }

    pub(super) fn stats(&self, member_count: usize) -> MemoryPoolStats {
        let mut free_extent_count = 0;
        let mut leased_extent_count = 0;
        let mut largest_free_extent = 0;

        for record in self.extents.iter().flatten() {
            match record.disposition {
                ExtentDisposition::Free => {
                    free_extent_count += 1;
                    largest_free_extent = largest_free_extent.max(record.range.len);
                }
                ExtentDisposition::Leased(_) => leased_extent_count += 1,
            }
        }
        let extent_slots_used = free_extent_count + leased_extent_count;
        let extent_slot_capacity = EXTENTS;
        let extent_slots_free = extent_slot_capacity.saturating_sub(extent_slots_used);

        MemoryPoolStats {
            total_bytes: self.free_bytes.saturating_add(self.leased_bytes),
            free_bytes: self.free_bytes,
            leased_bytes: self.leased_bytes,
            largest_free_extent,
            member_count,
            free_extent_count,
            leased_extent_count,
            extent_slot_capacity,
            extent_slots_used,
            extent_slots_free,
        }
    }
}
