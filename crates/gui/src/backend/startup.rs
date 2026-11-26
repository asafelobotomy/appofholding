//! What the window was started with, and the theme it starts in.

use super::*;

thread_local! {
    static LAST_UQ: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

pub(crate) fn startup_args() -> Vec<String> {
    std::env::args().skip(1).collect()
}

/// Files passed on the command line (e.g. by double-clicking an AppImage), as absolute paths.
pub fn startup_file_list() -> Vec<String> {
    crate::instance::parse_args(&startup_args())
        .0
        .iter()
        .map(|a| service::local_path(a).to_string_lossy().into_owned())
        .collect()
}

/// The app page asked for on the command line: `--app ID` or `--desktop-id
/// appofholding-ID.desktop` (as uQ's "Launch options…" app action passes), with `--tab`.
pub fn startup_open() -> Option<crate::instance::Open> {
    crate::instance::parse_args(&startup_args()).1
}

pub(crate) fn open_json(open: Option<&crate::instance::Open>) -> String {
    open.map_or(Value::Null, crate::instance::Open::to_json)
        .to_string()
}

pub fn started_in_tray() -> bool {
    std::env::args().skip(1).any(|a| a == service::TRAY_FLAG)
}

/// The saved colour theme, read before the interface starts (it decides the controls style).
pub fn current_theme() -> String {
    appofholding_core::Dirs::detect()
        .map(|d| service::theme_load(&d))
        .unwrap_or_else(|_| "uq".into())
}

/// Read uQ's tokens and, when they differ from the last ones applied, put them on the
/// application palette. Returns the palette JSON, or `None` without uQ's theme.
pub fn apply_uq_palette() -> Option<String> {
    let json = crate::uqtheme::load()?.to_string();
    LAST_UQ.with(|last| {
        if *last.borrow() != json {
            let c = std::ffi::CString::new(json.clone()).ok()?;
            // SAFETY: plain C function defined in theme.cpp; the application object exists.
            if !unsafe { crate::appofholding_set_uq_palette(c.as_ptr()) } {
                return None;
            }
            *last.borrow_mut() = json.clone();
        }
        Some(json)
    })
}
