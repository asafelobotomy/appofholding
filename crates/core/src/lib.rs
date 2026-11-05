//! appofholding core: AppImage parsing, library database and desktop integration.

pub mod assets;
pub mod check;
pub mod db;
pub mod delta;
pub mod desktop;
pub mod download;
mod elf;
pub mod engine;
pub mod error;
pub mod extract;
#[cfg(any(test, feature = "fixtures"))]
pub mod fixture;
pub mod http;
pub mod icon;
pub mod image;
pub mod launch;
pub mod library;
pub mod notify;
pub mod paths;
pub mod profiles;
pub mod providers;
pub mod running;
pub mod sandbox;
pub mod schedule;
pub mod source;
pub mod updates;
pub mod version;

pub use db::{AppRecord, SourceRecord};
pub use error::{Error, Result};
pub use library::{AddOptions, Install, Library, inspect};
pub use paths::Dirs;
