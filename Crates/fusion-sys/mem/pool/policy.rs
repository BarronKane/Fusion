/// Provisioning policy for building and growing a pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryPoolProvisioningPolicy {
    /// Only already-ready contributors may enter the pool.
    ReadyOnly,
    /// Present contributors may require legal state preparation before use.
    AllowPrepare,
    /// Provider-created contributors may be provisioned later.
    AllowProvision,
}

/// High-level policy controlling pool composition and behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemoryPoolPolicy {
    /// How much provider-side preparation or provisioning the pool build may rely on.
    pub provisioning: MemoryPoolProvisioningPolicy,
    /// Whether contributors from different topology nodes may be mixed deliberately.
    pub allow_cross_topology: bool,
}

impl MemoryPoolPolicy {
    /// Returns a deterministic, ready-only policy suitable for critical paths.
    #[must_use]
    pub const fn ready_only() -> Self {
        Self {
            provisioning: MemoryPoolProvisioningPolicy::ReadyOnly,
            allow_cross_topology: false,
        }
    }
}

impl Default for MemoryPoolPolicy {
    fn default() -> Self {
        Self::ready_only()
    }
}
