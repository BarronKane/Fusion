//! Integration-test canvas for `fusion_sys::alloc`.
//!
//! These tests cover the sanctioned allocation-facing pool path while the higher allocator
//! implementations are still being built on top of it.

mod all;
mod allocator_channel;
mod allocator_root;
mod arena;
mod retained;
mod slab;
mod support;
