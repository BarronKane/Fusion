#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceRange {
    pub offset: usize,
    pub len: usize,
}

impl ResourceRange {
    #[must_use]
    pub const fn new(offset: usize, len: usize) -> Self {
        Self { offset, len }
    }

    #[must_use]
    pub const fn whole(len: usize) -> Self {
        Self { offset: 0, len }
    }
}
