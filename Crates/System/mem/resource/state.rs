use fusion_pal::sys::mem::Protect;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceStateProvenance {
    Static,
    Tracked,
    Snapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StateValue<T> {
    Uniform(T),
    Asymmetric,
    Unknown,
}

impl<T> StateValue<T> {
    #[must_use]
    pub const fn uniform(value: T) -> Self {
        Self::Uniform(value)
    }

    #[must_use]
    pub const fn asymmetric() -> Self {
        Self::Asymmetric
    }

    #[must_use]
    pub const fn unknown() -> Self {
        Self::Unknown
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceState {
    pub provenance: ResourceStateProvenance,
    pub current_protect: StateValue<Protect>,
    pub locked: StateValue<bool>,
    pub committed: StateValue<bool>,
}

impl ResourceState {
    #[must_use]
    pub const fn tracked(protect: Protect, locked: bool, committed: bool) -> Self {
        Self {
            provenance: ResourceStateProvenance::Tracked,
            current_protect: StateValue::Uniform(protect),
            locked: StateValue::Uniform(locked),
            committed: StateValue::Uniform(committed),
        }
    }

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
