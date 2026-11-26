//! The window's own preferences: theme, view, desktop integration and the update schedule.

use super::*;

/// Colour themes the interface offers; the first, uQ's own, is the default and follows the
/// desktop's palette when uQ's theme is not there.
pub const THEMES: [&str; 4] = ["uq", "system", "gruvbox-dark", "gruvbox-light"];

/// A one-word interface setting kept in `~/.config/appofholding/<file>`, one of `choices`.
pub(crate) fn gui_file(dirs: &Dirs, file: &str) -> PathBuf {
    dirs.config_home.join("appofholding").join(file)
}

/// The saved choice; anything missing or unrecognised means the first one.
pub(crate) fn choice_load(dirs: &Dirs, file: &str, choices: &[&str]) -> String {
    std::fs::read_to_string(gui_file(dirs, file))
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| choices.contains(&t.as_str()))
        .unwrap_or_else(|| choices[0].to_string())
}

pub(crate) fn choice_save(
    dirs: &Dirs,
    file: &str,
    choices: &[&str],
    what: &str,
    name: &str,
) -> Res<()> {
    if !choices.contains(&name) {
        return Err(format!("unknown {what} '{name}'"));
    }
    let f = gui_file(dirs, file);
    std::fs::create_dir_all(f.parent().expect("has a parent")).map_err(e)?;
    std::fs::write(f, format!("{name}\n")).map_err(e)
}

/// The saved colour theme; anything missing or unrecognised means "uq".
pub fn theme_load(dirs: &Dirs) -> String {
    choice_load(dirs, "gui-theme-2", &THEMES)
}

pub fn theme_save(dirs: &Dirs, name: &str) -> Res<()> {
    choice_save(dirs, "gui-theme-2", &THEMES, "theme", name)
}

/// How the library lists apps; the first is the default.
pub const VIEWS: [&str; 3] = ["list", "grid", "compact"];

/// The saved library layout; anything missing or unrecognised means "list".
pub fn view_load(dirs: &Dirs) -> String {
    choice_load(dirs, "gui-view", &VIEWS)
}

pub fn view_save(dirs: &Dirs, name: &str) -> Res<()> {
    choice_save(dirs, "gui-view", &VIEWS, "view", name)
}

// ---- desktop integration: tray icon, starting with the session

/// How the window behaves on the desktop. Missing keys take these defaults.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopSettings {
    /// Show an icon in the system tray.
    pub tray: bool,
    /// Minimising the window hides it in the tray.
    pub minimise_to_tray: bool,
    /// Closing the window keeps appOFholding running in the tray.
    pub close_to_tray: bool,
    /// While in the tray, check for updates and announce new versions.
    pub notify_updates: bool,
    /// Started with the session: stay in the tray instead of opening the window.
    pub start_in_tray: bool,
    /// What to do with an AppImage dropped on the window or opened with appOFholding:
    /// `ask`, or one of the add modes (`move`, `copy`, `in-place`).
    pub drop_action: String,
}

/// The choices for `DesktopSettings::drop_action`; the first is the default.
pub const DROP_ACTIONS: [&str; 4] = ["ask", "move", "copy", "in-place"];

impl Default for DesktopSettings {
    fn default() -> Self {
        Self {
            tray: false,
            minimise_to_tray: false,
            close_to_tray: false,
            notify_updates: true,
            start_in_tray: true,
            drop_action: DROP_ACTIONS[0].to_string(),
        }
    }
}

impl DesktopSettings {
    pub(crate) fn to_json(&self) -> Value {
        json!({
            "tray": self.tray,
            "minimiseToTray": self.minimise_to_tray,
            "closeToTray": self.close_to_tray,
            "notifyUpdates": self.notify_updates,
            "startInTray": self.start_in_tray,
            "dropAction": self.drop_action,
        })
    }

    pub(crate) fn from_json(v: &Value) -> Self {
        let d = Self::default();
        let b = |k: &str, def: bool| v.get(k).and_then(Value::as_bool).unwrap_or(def);
        Self {
            tray: b("tray", d.tray),
            minimise_to_tray: b("minimiseToTray", d.minimise_to_tray),
            close_to_tray: b("closeToTray", d.close_to_tray),
            notify_updates: b("notifyUpdates", d.notify_updates),
            start_in_tray: b("startInTray", d.start_in_tray),
            drop_action: v
                .get("dropAction")
                .and_then(Value::as_str)
                .filter(|a| DROP_ACTIONS.contains(a))
                .map_or(d.drop_action, str::to_string),
        }
    }
}

pub(crate) fn desktop_file(dirs: &Dirs) -> PathBuf {
    gui_file(dirs, "gui-desktop.json")
}

/// The XDG autostart entry that starts appOFholding when the user logs in.
pub fn autostart_file(dirs: &Dirs) -> PathBuf {
    dirs.config_home
        .join("autostart")
        .join("io.github.appofholding.desktop")
}

/// The command-line flag for "start in the tray, without opening the window".
pub const TRAY_FLAG: &str = "--tray";

/// The settings plus whether the autostart entry exists, for the Settings page.
pub fn desktop_load(dirs: &Dirs) -> Value {
    let saved = std::fs::read_to_string(desktop_file(dirs))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .unwrap_or(Value::Null);
    let mut v = DesktopSettings::from_json(&saved).to_json();
    v["autostart"] = json!(autostart_file(dirs).exists());
    v
}

/// Save the settings; `autostart` in `v` writes or removes the login entry, which starts
/// `gui_exe` (with `--tray` when it should stay in the tray).
pub fn desktop_save(dirs: &Dirs, v: &Value, gui_exe: &Path) -> Res<()> {
    let s = DesktopSettings::from_json(v);
    let f = desktop_file(dirs);
    std::fs::create_dir_all(f.parent().expect("has a parent")).map_err(e)?;
    std::fs::write(&f, format!("{:#}\n", s.to_json())).map_err(e)?;
    let auto = autostart_file(dirs);
    if v.get("autostart").and_then(Value::as_bool).unwrap_or(false) {
        if !gui_exe.is_absolute() {
            return Err("cannot tell where appOFholding is installed".into());
        }
        std::fs::create_dir_all(auto.parent().expect("has a parent")).map_err(e)?;
        std::fs::write(&auto, autostart_text(gui_exe, s.tray && s.start_in_tray)).map_err(e)?;
    } else {
        match std::fs::remove_file(&auto) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(e(err)),
        }
    }
    Ok(())
}

pub fn autostart_text(gui_exe: &Path, in_tray: bool) -> String {
    let mut exec = desktop::quote_exec_arg(&gui_exe.to_string_lossy());
    if in_tray {
        exec.push(' ');
        exec.push_str(TRAY_FLAG);
    }
    format!(
        "[Desktop Entry]\nType=Application\nName=appOFholding\n\
         Comment=Keep AppImages up to date\nExec={exec}\nIcon=io.github.appofholding\n\
         Terminal=false\nX-GNOME-Autostart-enabled=true\nX-KDE-autostart-after=panel\n"
    )
}

/// The file that starts this interface again: the AppImage when running as one (this binary
/// is inside its temporary mount), otherwise this binary. After a package update the running
/// file was replaced, and `/proc/self/exe` reads "<path> (deleted)"; the path itself then
/// holds the new version.
pub fn gui_exe() -> Res<PathBuf> {
    if let Some(image) = std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        return Ok(image);
    }
    std::fs::read_link("/proc/self/exe")
        .or_else(|_| std::env::current_exe())
        .map(|p| {
            let s = p.to_string_lossy();
            PathBuf::from(s.strip_suffix(" (deleted)").unwrap_or(&s).to_string())
        })
        .map_err(e)
}

pub fn schedule_status(dirs: &Dirs) -> Value {
    json!({
        "installed": schedule::installed(dirs),
        "interval": schedule::installed_interval(dirs).map(Interval::as_str),
    })
}

pub fn schedule_install(dirs: &Dirs, interval: &str) -> Res<()> {
    let iv = Interval::parse(interval).ok_or("interval must be hourly, daily or weekly")?;
    if !schedule::enable(dirs, &cli_path(), iv).map_err(e)? {
        return Err("wrote the timer files but systemd would not enable them".into());
    }
    Ok(())
}

pub fn schedule_remove(dirs: &Dirs) -> Res<()> {
    schedule::disable(dirs).map(|_| ()).map_err(e)
}
