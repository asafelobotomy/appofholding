//! appimgr core: AppImage parsing, library database and desktop integration.

pub mod assets;
pub mod check;
pub mod db;
pub mod desktop;
pub mod download;
mod elf;
pub mod error;
#[cfg(any(test, feature = "fixtures"))]
pub mod fixture;
pub mod http;
pub mod icon;
pub mod image;
pub mod library;
pub mod paths;
pub mod providers;
pub mod source;
pub mod updates;
pub mod version;

pub use db::{AppRecord, SourceRecord};
pub use error::{Error, Result};
pub use library::{AddOptions, Install, Library, inspect};
pub use paths::Dirs;
