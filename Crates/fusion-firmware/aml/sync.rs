//! AML synchronization and serialization vocabulary.

/// Method serialization mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AmlMethodSerialization {
    NotSerialized,
    Serialized,
}

/// AML mutex identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmlMutexDescriptor {
    pub sync_level: u8,
}
