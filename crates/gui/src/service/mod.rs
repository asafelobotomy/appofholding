//! Everything the interface does, as plain functions over the core library that return JSON
//! (or an error string). No Qt in here, so it is fully unit-testable; the QObject in
//! `backend.rs` only forwards calls and moves slow ones onto threads.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use appofholding_core::assets::Selection;
use appofholding_core::engine::{self, Applied, ApplyOptions, IfRunning};
use appofholding_core::http::UreqHttp;
use appofholding_core::launch::{self, DEFAULT_PROFILE, Profile};
use appofholding_core::profiles::LaunchCtx;
use appofholding_core::sandbox::{Bus, HomeAccess, Level, PathGrant, Settings};
use appofholding_core::schedule::{self, Interval};
use appofholding_core::source::{self, Channel, Policy};
use appofholding_core::{AddOptions, AppRecord, Dirs, Install, Library, desktop};
use serde_json::{Value, json};

mod apps;
mod forms;
mod install;
mod prefs;
mod updates;

pub use apps::*;
pub use forms::*;
pub use install::*;
pub use prefs::*;
pub use updates::*;

pub type Res<T> = std::result::Result<T, String>;

fn e<E: std::fmt::Display>(err: E) -> String {
    err.to_string()
}

/// The `appofholding` command-line binary that generated menu entries call. It lives next to
/// the GUI binary when both are installed together.
pub fn cli_path() -> PathBuf {
    // Running as an AppImage: the AppImage file passes `run`/`update` on to the CLI inside it.
    if let Ok(exe) = std::env::current_exe() {
        let stable = appofholding_core::paths::stable_exec_path(exe.clone());
        if stable != exe {
            return stable;
        }
    }
    let sibling = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("appofholding")))
        .filter(|p| p.exists());
    sibling.unwrap_or_else(|| {
        std::env::var_os("PATH")
            .and_then(|p| {
                std::env::split_paths(&p)
                    .map(|d| d.join("appofholding"))
                    .find(|c| c.exists())
            })
            .unwrap_or_else(|| PathBuf::from("/usr/bin/appofholding"))
    })
}

pub fn open_lib() -> Res<Library> {
    Library::open(Dirs::detect().map_err(e)?, cli_path()).map_err(e)
}

#[cfg(test)]
mod tests;
