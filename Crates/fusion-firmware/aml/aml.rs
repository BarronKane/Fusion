//! Fusion AML frontend, loader, lowering, and VM substrate.
//!
//! This module is the firmware-side answer to ACPI's bytecode problem:
//! - not ACPICA,
//! - not Rust-function lowering theater,
//! - not table parsing pretending to be runtime behavior.
//!
//! The shape here is deliberately complete even though implementation is still early:
//! - bytecode and definition-block envelope
//! - parser and namespace loader
//! - object/value/reference model
//! - method evaluation context
//! - opregion/field host boundary
//! - trace/notify/synchronization vocabulary
//! - lowering seam toward `fusion-pcu` execution models
//!
//! That keeps the architecture honest while the actual interpreter grows a piece at a time.

mod bytecode;
mod context;
mod error;
mod eval;
mod field;
mod host;
mod loader;
mod lowering;
mod method;
mod name;
mod namespace;
mod notify;
mod object;
mod opregion;
mod parser;
mod reference;
mod state;
mod sync;
mod trace;
mod value;
mod verify;
mod vm;

pub use bytecode::*;
pub use context::*;
pub use error::*;
pub use eval::*;
pub use field::*;
pub use host::*;
pub use loader::*;
pub use lowering::*;
pub use method::*;
pub use name::*;
pub use namespace::*;
pub use notify::*;
pub use object::*;
pub use opregion::*;
pub use parser::*;
pub use reference::*;
pub use sync::*;
pub use state::*;
pub use trace::*;
pub use value::*;
pub use verify::*;
pub use vm::*;
