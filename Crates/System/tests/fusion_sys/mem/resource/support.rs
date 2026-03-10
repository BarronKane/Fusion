//! Shared helpers for `fusion_sys::mem::resource` integration tests.

use fusion_pal::sys::mem::{MemBase, system_mem};

/// Returns a multiple of the backend's base page size.
///
/// The tests use page-aligned lengths by default so they exercise the resource layer without
/// also testing misalignment rejection unless a test explicitly intends to do that.
pub fn page_len(multiplier: usize) -> usize {
    system_mem().page_info().base_page.get() * multiplier
}
