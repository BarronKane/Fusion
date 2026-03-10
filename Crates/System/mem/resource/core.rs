use core::cell::Cell;

use fusion_pal::sys::mem::Protect;

use super::{ResolvedResource, ResourceInfo, ResourceState, StateValue};

/// Shared composed storage for immutable resource info and mutable state summary.
#[derive(Debug)]
pub struct ResourceCore {
    resolved: ResolvedResource,
    state: Cell<ResourceState>,
}

impl ResourceCore {
    /// Creates a new composed core from resolved metadata and an initial state summary.
    #[must_use]
    pub const fn new(resolved: ResolvedResource, state: ResourceState) -> Self {
        Self {
            resolved,
            state: Cell::new(state),
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
    #[allow(clippy::missing_const_for_fn)]
    pub fn state(&self) -> ResourceState {
        self.state.get()
    }

    /// Records a uniform protection state for the full resource.
    pub fn set_current_protect(&self, protect: Protect) {
        let mut state = self.state.get();
        state.current_protect = StateValue::Uniform(protect);
        self.state.set(state);
    }

    /// Marks protection state as non-uniform across the resource.
    pub fn mark_protect_asymmetric(&self) {
        let mut state = self.state.get();
        state.current_protect = StateValue::Asymmetric;
        self.state.set(state);
    }

    /// Records a uniform lock state for the full resource.
    pub fn set_locked_state(&self, locked: bool) {
        let mut state = self.state.get();
        state.locked = StateValue::Uniform(locked);
        self.state.set(state);
    }

    /// Marks lock state as non-uniform across the resource.
    pub fn mark_locked_asymmetric(&self) {
        let mut state = self.state.get();
        state.locked = StateValue::Asymmetric;
        self.state.set(state);
    }

    /// Records a uniform commitment state for the full resource.
    pub fn set_committed_state(&self, committed: bool) {
        let mut state = self.state.get();
        state.committed = StateValue::Uniform(committed);
        self.state.set(state);
    }

    /// Marks commitment state as non-uniform across the resource.
    pub fn mark_committed_asymmetric(&self) {
        let mut state = self.state.get();
        state.committed = StateValue::Asymmetric;
        self.state.set(state);
    }
}
