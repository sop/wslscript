#[macro_use]
extern crate failure;

pub mod error;
pub mod icon;
pub mod registry;
pub mod win32;
pub mod wsl;

pub use registry::DROP_HANDLER_CLSID;
pub use win32::{wcstr, wcstring};
