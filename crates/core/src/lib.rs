//! appimgr core: AppImage parsing, library database and desktop integration.

pub mod db;
pub mod desktop;
mod elf;
pub mod error;
#[cfg(any(test, feature = "fixtures"))]
pub mod fixture;
pub mod icon;
pub mod image;
pub mod library;
pub mod paths;

pub use db::AppRecord;
pub use error::{Error, Result};
pub use library::{AddOptions, Install, Library, inspect};
pub use paths::Dirs;
