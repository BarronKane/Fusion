/// Byte range relative to the base of a memory resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceRange {
    /// Byte offset from the start of the resource.
    pub offset: usize,
    /// Length of the range in bytes.
    pub len: usize,
}

impl ResourceRange {
    /// Creates a range with an explicit byte offset and length.
    #[must_use]
    pub const fn new(offset: usize, len: usize) -> Self {
        Self { offset, len }
    }

    /// Creates a range that covers a full resource of `len` bytes.
    #[must_use]
    pub const fn whole(len: usize) -> Self {
        Self { offset: 0, len }
    }
}
