//! Display driver families.
//!
//! Concrete display-driver implementations live in external `fd-*` crates such as:
//! - `fd-display-layout`
//! - `fd-display-port-hdmi`
//! - `fd-display-port-dvi`
//! - `fd-display-port-vga`
//! - `fd-display-port-display_port`
//!
//! `fusion-hal` keeps:
//! - the display contracts
//! - the family/doctrine lane
//! - shared internal display-driver helper law reused by the concrete crates

#[path = "layout.rs"]
pub mod layout;

#[path = "port.rs"]
pub mod port;

#[doc(hidden)]
#[path = "shared/shared.rs"]
pub mod shared;
