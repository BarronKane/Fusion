use crate::mem::resource::AllocatorLayoutPolicy;
use super::{
    AllocError,
    align_up,
};

/// Allocator-managed subsystem kind that owns one front-loaded metadata region.
#[repr(u16)]
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AllocSubsystemKind {
    Slab = 1,
    BoundedArena = 2,
    Arena = 3,
    Heap = 4,
    Control = 5,
}

/// Common header written into the front metadata region of one allocator subsystem extent.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MetadataPageHeader {
    pub subsystem: AllocSubsystemKind,
    pub metadata_len: usize,
    pub payload_offset: usize,
    pub payload_len: usize,
    pub overflow_next: usize,
}

impl MetadataPageHeader {
    #[must_use]
    pub const fn new(
        subsystem: AllocSubsystemKind,
        metadata_len: usize,
        payload_offset: usize,
        payload_len: usize,
    ) -> Self {
        Self {
            subsystem,
            metadata_len,
            payload_offset,
            payload_len,
            overflow_next: 0,
        }
    }
}

/// Concrete front-loaded metadata and payload layout for one subsystem extent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrontMetadataLayout {
    pub metadata_len: usize,
    pub payload_offset: usize,
    pub payload_len: usize,
    pub total_len: usize,
    pub request_align: usize,
}

/// Computes the root extent layout for one allocator subsystem with one explicit allocator layout
/// policy.
pub fn front_metadata_layout_with_policy(
    header_bytes: usize,
    header_align: usize,
    payload_len: usize,
    payload_align: usize,
    layout_policy: AllocatorLayoutPolicy,
) -> Result<FrontMetadataLayout, AllocError> {
    if header_bytes == 0
        || header_align == 0
        || payload_len == 0
        || payload_align == 0
        || !header_align.is_power_of_two()
        || !payload_align.is_power_of_two()
    {
        return Err(AllocError::invalid_request());
    }

    let metadata_granule = layout_policy.metadata_granule.get();
    let request_align = layout_policy
        .min_extent_align
        .get()
        .max(header_align)
        .max(payload_align);
    let metadata_len = align_up(header_bytes, metadata_granule)?;
    let payload_offset = align_up(metadata_len, payload_align)?;
    let total_len = payload_offset
        .checked_add(payload_len)
        .ok_or_else(AllocError::invalid_request)?;

    Ok(FrontMetadataLayout {
        metadata_len,
        payload_offset,
        payload_len,
        total_len,
        request_align,
    })
}
