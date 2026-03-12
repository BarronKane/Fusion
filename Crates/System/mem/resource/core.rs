//! Thread-safe composed storage for immutable resource metadata and mutable summary state.
//!
//! Resource state is intentionally summarized, not page-tracked. Readers observe it through an
//! atomic snapshot, while backing-mutating operations serialize through a thin mutex so
//! concurrent allocator maintenance does not corrupt resource-wide truth claims.
//!
//! The current mutex path is chosen through `fusion-sys::sync::ThinMutex`, which prefers a
//! PAL-backed native mutex where available and falls back to a small spin mutex elsewhere.
//! That keeps the runtime panic-free and no-alloc today without pretending every target already
//! exposes the same native synchronization substrate.

use core::sync::atomic::{AtomicU32, Ordering};

use crate::sync::{ThinMutex, ThinMutexGuard};
use fusion_pal::sys::mem::Protect;

use super::{ResolvedResource, ResourceInfo, ResourceState, ResourceStateProvenance, StateValue};

const PROVENANCE_SHIFT: u32 = 0;
const PROTECT_TAG_SHIFT: u32 = 2;
const PROTECT_BITS_SHIFT: u32 = 4;
const LOCKED_SHIFT: u32 = 8;
const COMMITTED_SHIFT: u32 = 10;

const PROTECT_MASK: u32 = 0xF;
const SUMMARY_MASK: u32 = 0x3;

const SUMMARY_UNIFORM: u32 = 0;
const SUMMARY_ASYMMETRIC: u32 = 1;
const SUMMARY_UNKNOWN: u32 = 2;

const BOOL_FALSE: u32 = 0;
const BOOL_TRUE: u32 = 1;
const BOOL_ASYMMETRIC: u32 = 2;
const BOOL_UNKNOWN: u32 = 3;

/// Shared composed storage for immutable resource info and thread-safe mutable state summary.
///
/// Summary state is published atomically so readers can inspect it without locks. Backing-
/// mutating operations additionally take a thin mutex so full-resource state transitions cannot
/// race each other into fiction.
#[derive(Debug)]
pub struct ResourceCore {
    resolved: ResolvedResource,
    state_bits: AtomicU32,
    mutation_lock: ThinMutex,
}

impl ResourceCore {
    /// Creates a new composed core from resolved metadata and an initial state summary.
    #[must_use]
    pub const fn new(resolved: ResolvedResource, state: ResourceState) -> Self {
        Self {
            resolved,
            state_bits: AtomicU32::new(encode_state(state)),
            mutation_lock: ThinMutex::new(),
        }
    }

    /// Returns the immutable descriptive information for the resource.
    #[must_use]
    pub const fn info(&self) -> &ResourceInfo {
        &self.resolved.info
    }

    /// Returns the full creation-time resolution record.
    #[must_use]
    pub const fn resolved(&self) -> ResolvedResource {
        self.resolved
    }

    /// Returns the current summary state.
    #[must_use]
    pub fn state(&self) -> ResourceState {
        decode_state(self.state_bits.load(Ordering::Acquire))
    }

    /// Serializes a backing-mutating operation against other mutating operations on this
    /// resource.
    pub fn begin_mutation(&self) -> Result<ResourceMutationGuard<'_>, crate::sync::SyncError> {
        Ok(ResourceMutationGuard {
            core: self,
            _lock: self.mutation_lock.lock()?,
        })
    }

    fn update_state(&self, update: impl Fn(ResourceState) -> ResourceState) {
        let mut current = self.state_bits.load(Ordering::Acquire);
        loop {
            let next = encode_state(update(decode_state(current)));
            match self.state_bits.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return,
                Err(observed) => current = observed,
            }
        }
    }
}

/// Guard returned while a resource mutation is serialized.
///
/// Summary-state mutation methods live on this guard so callers cannot update resource-wide
/// tracked state without first acquiring the mutation serialization path.
#[must_use]
pub struct ResourceMutationGuard<'a> {
    core: &'a ResourceCore,
    _lock: ThinMutexGuard<'a>,
}

impl ResourceMutationGuard<'_> {
    /// Records a uniform protection state for the full resource.
    pub fn set_current_protect(&self, protect: Protect) {
        self.core.update_state(|mut state| {
            state.current_protect = StateValue::Uniform(protect);
            state
        });
    }

    /// Marks protection state as non-uniform across the resource.
    pub fn mark_protect_asymmetric(&self) {
        self.core.update_state(|mut state| {
            state.current_protect = StateValue::Asymmetric;
            state
        });
    }

    /// Records a uniform lock state for the full resource.
    pub fn set_locked_state(&self, locked: bool) {
        self.core.update_state(|mut state| {
            state.locked = StateValue::Uniform(locked);
            state
        });
    }

    /// Marks lock state as non-uniform across the resource.
    pub fn mark_locked_asymmetric(&self) {
        self.core.update_state(|mut state| {
            state.locked = StateValue::Asymmetric;
            state
        });
    }

    /// Records a uniform commitment state for the full resource.
    pub fn set_committed_state(&self, committed: bool) {
        self.core.update_state(|mut state| {
            state.committed = StateValue::Uniform(committed);
            state
        });
    }

    /// Marks commitment state as non-uniform across the resource.
    pub fn mark_committed_asymmetric(&self) {
        self.core.update_state(|mut state| {
            state.committed = StateValue::Asymmetric;
            state
        });
    }
}

const fn encode_state(state: ResourceState) -> u32 {
    encode_provenance(state.provenance)
        | encode_protect_state(state.current_protect)
        | encode_bool_state(state.locked, LOCKED_SHIFT)
        | encode_bool_state(state.committed, COMMITTED_SHIFT)
}

const fn encode_provenance(provenance: ResourceStateProvenance) -> u32 {
    let encoded = match provenance {
        ResourceStateProvenance::Static => 0,
        ResourceStateProvenance::Tracked => 1,
        ResourceStateProvenance::Snapshot => 2,
    };
    encoded << PROVENANCE_SHIFT
}

const fn encode_protect_state(state: StateValue<Protect>) -> u32 {
    match state {
        StateValue::Uniform(protect) => {
            (SUMMARY_UNIFORM << PROTECT_TAG_SHIFT)
                | ((protect.bits() & PROTECT_MASK) << PROTECT_BITS_SHIFT)
        }
        StateValue::Asymmetric => SUMMARY_ASYMMETRIC << PROTECT_TAG_SHIFT,
        StateValue::Unknown => SUMMARY_UNKNOWN << PROTECT_TAG_SHIFT,
    }
}

const fn encode_bool_state(state: StateValue<bool>, shift: u32) -> u32 {
    let encoded = match state {
        StateValue::Uniform(false) => BOOL_FALSE,
        StateValue::Uniform(true) => BOOL_TRUE,
        StateValue::Asymmetric => BOOL_ASYMMETRIC,
        StateValue::Unknown => BOOL_UNKNOWN,
    };
    encoded << shift
}

const fn decode_state(bits: u32) -> ResourceState {
    ResourceState {
        provenance: decode_provenance(bits),
        current_protect: decode_protect_state(bits),
        locked: decode_bool_state(bits, LOCKED_SHIFT),
        committed: decode_bool_state(bits, COMMITTED_SHIFT),
    }
}

const fn decode_provenance(bits: u32) -> ResourceStateProvenance {
    match bits & SUMMARY_MASK {
        0 => ResourceStateProvenance::Static,
        1 => ResourceStateProvenance::Tracked,
        _ => ResourceStateProvenance::Snapshot,
    }
}

const fn decode_protect_state(bits: u32) -> StateValue<Protect> {
    match (bits >> PROTECT_TAG_SHIFT) & SUMMARY_MASK {
        SUMMARY_UNIFORM => StateValue::Uniform(Protect::from_bits_retain(
            (bits >> PROTECT_BITS_SHIFT) & PROTECT_MASK,
        )),
        SUMMARY_ASYMMETRIC => StateValue::Asymmetric,
        _ => StateValue::Unknown,
    }
}

const fn decode_bool_state(bits: u32, shift: u32) -> StateValue<bool> {
    match (bits >> shift) & SUMMARY_MASK {
        BOOL_FALSE => StateValue::Uniform(false),
        BOOL_TRUE => StateValue::Uniform(true),
        BOOL_ASYMMETRIC => StateValue::Asymmetric,
        _ => StateValue::Unknown,
    }
}
