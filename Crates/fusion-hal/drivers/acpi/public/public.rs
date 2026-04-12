//! Canonical public ACPI driver families.
//!
//! These modules define the stable driver tier that higher layers should target. Vendor-specific
//! AML or EC glue lives underneath them as backend realizers.

pub mod battery;
pub mod button;
pub mod embedded_controller;
pub mod fan;
#[path = "interface/interface.rs"]
pub mod interface;
pub mod lid;
pub mod power_source;
pub mod processor;
pub mod thermal;
mod unsupported;
