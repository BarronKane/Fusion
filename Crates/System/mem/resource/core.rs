use core::cell::Cell;

use fusion_pal::sys::mem::Protect;

use super::{ResolvedResource, ResourceInfo, ResourceState, StateValue};

#[derive(Debug)]
pub struct ResourceCore {
    resolved: ResolvedResource,
    state: Cell<ResourceState>,
}

impl ResourceCore {
    #[must_use]
    pub const fn new(resolved: ResolvedResource, state: ResourceState) -> Self {
        Self {
            resolved,
            state: Cell::new(state),
        }
    }

    #[must_use]
    pub const fn info(&self) -> &ResourceInfo {
        &self.resolved.info
    }

    #[must_use]
    pub const fn resolved(&self) -> ResolvedResource {
        self.resolved
    }

    #[must_use]
    pub fn state(&self) -> ResourceState {
        self.state.get()
    }

    pub fn set_current_protect(&self, protect: Protect) {
        let mut state = self.state.get();
        state.current_protect = StateValue::Uniform(protect);
        self.state.set(state);
    }

    pub fn mark_protect_asymmetric(&self) {
        let mut state = self.state.get();
        state.current_protect = StateValue::Asymmetric;
        self.state.set(state);
    }

    pub fn set_locked_state(&self, locked: bool) {
        let mut state = self.state.get();
        state.locked = StateValue::Uniform(locked);
        self.state.set(state);
    }

    pub fn mark_locked_asymmetric(&self) {
        let mut state = self.state.get();
        state.locked = StateValue::Asymmetric;
        self.state.set(state);
    }

    pub fn set_committed_state(&self, committed: bool) {
        let mut state = self.state.get();
        state.committed = StateValue::Uniform(committed);
        self.state.set(state);
    }

    pub fn mark_committed_asymmetric(&self) {
        let mut state = self.state.get();
        state.committed = StateValue::Asymmetric;
        self.state.set(state);
    }
}
