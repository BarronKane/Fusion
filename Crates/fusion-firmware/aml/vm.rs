//! AML VM configuration and state vocabulary.

/// VM configuration knobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlVmConfig {
    pub enable_tracing: bool,
    pub allow_external_resolution: bool,
    pub eager_region_registration: bool,
}

impl Default for AmlVmConfig {
    fn default() -> Self {
        Self {
            enable_tracing: false,
            allow_external_resolution: false,
            eager_region_registration: true,
        }
    }
}

/// Coarse AML VM state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AmlVmState {
    Empty,
    Loaded,
    Ready,
    Running,
    Blocked,
}

/// Opaque AML VM anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlVm {
    pub state: AmlVmState,
}
