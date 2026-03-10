use fusion_pal::sys::mem::Protect;

/// Provenance of the current resource state summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceStateProvenance {
    /// The provider supplied a static description that is not expected to self-update.
    Static,
    /// The resource actively tracks its own state transitions.
    Tracked,
    /// The state came from a best-effort query snapshot.
    Snapshot,
}

/// Per-property summary for a resource-wide state value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StateValue<T> {
    /// The property is uniform across the entire resource.
    Uniform(T),
    /// The property differs across subranges of the resource.
    Asymmetric,
    /// The resource cannot currently prove a resource-wide answer.
    Unknown,
}

impl<T> StateValue<T> {
    /// Creates a uniform summary value.
    #[must_use]
    pub const fn uniform(value: T) -> Self {
        Self::Uniform(value)
    }

    /// Creates an explicitly asymmetric summary value.
    #[must_use]
    pub const fn asymmetric() -> Self {
        Self::Asymmetric
    }

    /// Creates an unknown summary value.
    #[must_use]
    pub const fn unknown() -> Self {
        Self::Unknown
    }
}

/// Resource-wide runtime state summary.
///
/// This type intentionally summarizes the whole resource rather than tracking every page or
/// subrange. TODO: A future state-tracking subsystem may retain finer-grained history and then
/// collapse it back into this summary form when higher layers only need a resource-wide view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceState {
    /// How this summary was obtained.
    pub provenance: ResourceStateProvenance,
    /// Resource-wide summary of the current protection state.
    pub current_protect: StateValue<Protect>,
    /// Resource-wide summary of lock state.
    pub locked: StateValue<bool>,
    /// Resource-wide summary of commitment state.
    pub committed: StateValue<bool>,
}

impl ResourceState {
    /// Creates a tracked state with uniform values across the whole resource.
    #[must_use]
    pub const fn tracked(protect: Protect, locked: bool, committed: bool) -> Self {
        Self {
            provenance: ResourceStateProvenance::Tracked,
            current_protect: StateValue::Uniform(protect),
            locked: StateValue::Uniform(locked),
            committed: StateValue::Uniform(committed),
        }
    }

    /// Creates a snapshot-derived summary state.
    #[must_use]
    pub const fn snapshot(
        current_protect: StateValue<Protect>,
        locked: StateValue<bool>,
        committed: StateValue<bool>,
    ) -> Self {
        Self {
            provenance: ResourceStateProvenance::Snapshot,
            current_protect,
            locked,
            committed,
        }
    }

    /// Creates a statically described summary state.
    #[must_use]
    pub const fn static_state(
        current_protect: StateValue<Protect>,
        locked: StateValue<bool>,
        committed: StateValue<bool>,
    ) -> Self {
        Self {
            provenance: ResourceStateProvenance::Static,
            current_protect,
            locked,
            committed,
        }
    }
}
