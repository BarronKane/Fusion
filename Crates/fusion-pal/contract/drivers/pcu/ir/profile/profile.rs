//! Profile/dialect layers composed over the PCU IR core.

#[path = "dispatch.rs"]
mod dispatch;
#[path = "stream.rs"]
mod stream;

pub use dispatch::*;
pub use stream::*;
