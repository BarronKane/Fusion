//! Integration-test canvas for `fusion_sys::mem::provider`.
//!
//! These tests focus on the provider contract shape rather than a platform backend. The
//! provider layer is the orchestration seam above concrete resources, so the tests here are
//! mostly about compatibility classification, safety filtering, and pool assessment.

mod all;
