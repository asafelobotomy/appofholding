//! Everything the interface does, as plain functions over the core library that return JSON
//! (or an error string). No Qt in here, so it is fully unit-testable; the QObject in
//! `backend.rs` only forwards calls and moves slow ones onto threads.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use appofholding_core::assets::Selection;
use appofholding_core::engine::{Applied, ApplyOptions, IfRunning};
use appofholding_core::http::UreqHttp;
use appofholding_core::launch::{self, DEFAULT_PROFILE, Profile};
use appofholding_core::profiles::LaunchCtx;
use appofholding_core::sandbox::{Bus, HomeAccess, Level, PathGrant, Settings};
use appofholding_core::schedule::{self, Interval};
use appofholding_core::source::{self, Channel, Policy};
use appofholding_core::{AddOptions, AppRecord, Dirs, Install, Library, desktop};
use serde_json::{Value, json};

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

fn icon_url(a: &AppRecord) -> String {
    a.icon_file
        .as_ref()
        .map(|p| format!("file://{}", p.display()))
        .unwrap_or_default()
}

/// Where updates come from, short enough for a list row.
fn short_source(p: &source::Provider) -> String {
    let host = |url: &str| {
        url.split("//")
            .nth(1)
            .and_then(|r| r.split('/').next())
            .unwrap_or(url)
            .to_string()
    };
    match p {
        source::Provider::GitHub { owner, repo } => format!("github.com/{owner}/{repo}"),
        source::Provider::GitLab { host, project } => format!("{host}/{project}"),
        source::Provider::Forgejo { host, owner, repo } => format!("{host}/{owner}/{repo}"),
        source::Provider::Url { url } | source::Provider::Zsync { url } => host(url),
    }
}

pub fn list_apps(lib: &Library) -> Res<Value> {
    let sources: std::collections::HashMap<_, _> = lib
        .sources()
        .map_err(e)?
        .into_iter()
        .map(|s| (s.app_id.clone(), s))
        .collect();
    let sandboxes = lib.all_sandbox_settings().map_err(e)?;
    let mut out = Vec::new();
    for a in lib.list().map_err(e)? {
        let src = sources.get(&a.id);
        let sandbox = sandboxes.get(&a.id).cloned().unwrap_or_default();
        out.push(json!({
            "id": a.id,
            "name": a.name,
            "version": a.version.clone().or(src.and_then(|s| s.installed_version.clone())),
            "icon": icon_url(&a),
            "sandbox": sandbox.level.as_str(),
            "hasSource": src.is_some(),
            "source": src.map(|s| short_source(&s.source.provider)),
            "lastChecked": src.and_then(|s| s.last_checked),
            "status": src.and_then(|s| s.last_status.clone()).unwrap_or_default(),
            "latest": src.and_then(|s| s.last_latest.clone()),
            "error": src.and_then(|s| s.last_error.clone()),
            "policy": src.map(|s| s.policy.as_str()).unwrap_or(""),
        }));
    }
    Ok(Value::Array(out))
}

fn profile_json(p: &Profile) -> Value {
    json!({
        "args": desktop::join_exec(&p.args),
        "env": p.env.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("\n"),
        "prefix": desktop::join_exec(&p.prefix),
        "cwd": p.cwd.as_ref().map(|c| c.display().to_string()).unwrap_or_default(),
        "presets": p.presets,
        "extractAndRun": p.extract_and_run,
        "elevated": p.elevated,
    })
}

pub fn app_detail(lib: &Library, id: &str) -> Res<Value> {
    let a = lib.get(id).map_err(e)?;
    let src = lib.source(id).map_err(e)?;
    let sandbox = lib.sandbox_settings(id).map_err(e)?;
    let profiles = lib.profiles(id).map_err(e)?;
    let default_profile = lib.profile(id, DEFAULT_PROFILE).map_err(e)?;
    let mut names: Vec<String> = vec![DEFAULT_PROFILE.to_string()];
    names.extend(
        profiles
            .iter()
            .map(|(n, _)| n.clone())
            .filter(|n| n != DEFAULT_PROFILE),
    );
    Ok(json!({
        "id": a.id,
        "name": a.name,
        "version": a.version.clone().or(src.as_ref().and_then(|s| s.installed_version.clone())),
        "path": a.appimage_path.display().to_string(),
        "size": a.size,
        "sha256": a.sha256,
        "desktopFile": a.desktop_file.display().to_string(),
        "externalFile": a.external_file,
        "icon": icon_url(&a),
        "running": !appofholding_core::running::running_pids(&a.appimage_path).is_empty(),
        "source": src.as_ref().map(|s| json!({
            "describe": s.source.describe(),
            "link": appofholding_core::config::source_to_input(&s.source.provider),
            "origin": if s.source.origin == source::Origin::Embedded { "embedded" } else { "user" },
            "pattern": s.source.asset_pattern,
            "channel": s.source.channel.to_string(),
            "installed": s.installed_version.clone().or(a.version.clone()),
            "lastChecked": s.last_checked,
            "status": s.last_status,
            "latest": s.last_latest,
            "error": s.last_error,
            "policy": s.policy.as_str(),
            "skipped": s.skipped_version,
        })),
        "versions": lib.versions(id).map_err(e)?.iter().map(|v| json!({
            "version": v.version,
            "sha": &v.sha256[..v.sha256.len().min(12)],
            "key": v.version.clone().unwrap_or_else(|| v.sha256[..8].to_string()),
        })).collect::<Vec<_>>(),
        "profiles": names,
        "launch": profile_json(&default_profile),
        "sandbox": sandbox_json(&sandbox),
        "hooks": lib.hooks(id).map_err(e)?.into_iter().map(|(k, c)| json!({"kind": k, "command": c})).collect::<Vec<_>>(),
    }))
}

pub fn profile_detail(lib: &Library, id: &str, name: &str) -> Res<Value> {
    Ok(profile_json(&lib.profile(id, name).map_err(e)?))
}

fn sandbox_json(s: &Settings) -> Value {
    json!({
        "level": s.level.as_str(),
        "network": s.network,
        "home": s.home.as_str(),
        "gpu": s.gpu,
        "audio": s.audio,
        "devices": s.devices,
        "display": s.display,
        "bus": s.bus.as_str(),
        "chromium": s.chromium_workaround,
        "paths": s.paths.iter().map(|g| json!({"path": g.path.display().to_string(), "write": g.write})).collect::<Vec<_>>(),
    })
}

pub fn sandbox_defaults(level: &str) -> Res<Value> {
    let l = Level::parse(level).ok_or("unknown level")?;
    Ok(sandbox_json(&Settings::for_level(l)))
}

pub fn presets() -> Value {
    Value::Array(
        launch::PRESETS
            .iter()
            .map(|p| json!({"name": p.name, "description": p.description}))
            .collect(),
    )
}

fn str_of<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}
fn bool_of(v: &Value, k: &str) -> bool {
    v.get(k).and_then(Value::as_bool).unwrap_or(false)
}

pub fn parse_profile(v: &Value) -> Res<Profile> {
    let mut p = Profile {
        args: desktop::split_exec(str_of(v, "args")),
        prefix: desktop::split_exec(str_of(v, "prefix")),
        extract_and_run: bool_of(v, "extractAndRun"),
        elevated: bool_of(v, "elevated"),
        ..Profile::default()
    };
    for line in str_of(v, "env")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
    {
        let (k, val) = line
            .split_once('=')
            .ok_or_else(|| format!("'{line}' should look like NAME=value"))?;
        p.env.insert(k.trim().to_string(), val.to_string());
    }
    let cwd = str_of(v, "cwd").trim();
    if !cwd.is_empty() {
        p.cwd = Some(PathBuf::from(cwd));
    }
    if let Some(a) = v.get("presets").and_then(Value::as_array) {
        p.presets = a
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
    }
    Ok(p)
}

pub fn save_profile(lib: &Library, id: &str, name: &str, v: &Value) -> Res<()> {
    let p = parse_profile(v)?;
    lib.set_profile(id, name, &p).map_err(e)
}

pub fn delete_profile(lib: &Library, id: &str, name: &str) -> Res<()> {
    lib.delete_profile(id, name).map(|_| ()).map_err(e)
}

pub fn parse_sandbox(v: &Value) -> Res<Settings> {
    let level = Level::parse(str_of(v, "level")).ok_or("unknown sandbox level")?;
    let mut s = Settings::for_level(level);
    if level != Level::None {
        s.network = bool_of(v, "network");
        s.gpu = bool_of(v, "gpu");
        s.audio = bool_of(v, "audio");
        s.devices = bool_of(v, "devices");
        s.display = bool_of(v, "display");
        s.chromium_workaround = bool_of(v, "chromium");
        s.home = HomeAccess::parse(str_of(v, "home")).ok_or("home must be private, ro or rw")?;
        s.bus = Bus::parse(str_of(v, "bus")).ok_or("bus must be off, filtered or full")?;
    }
    if let Some(a) = v.get("paths").and_then(Value::as_array) {
        s.paths = a
            .iter()
            .map(|g| PathGrant {
                path: PathBuf::from(str_of(g, "path")),
                write: bool_of(g, "write"),
            })
            .collect();
    }
    Ok(s)
}

pub fn save_sandbox(lib: &Library, id: &str, v: &Value) -> Res<Value> {
    let s = parse_sandbox(v)?;
    Ok(sandbox_json(&lib.set_sandbox(id, &s).map_err(e)?))
}

pub fn preview(lib: &Library, id: &str, profile: &str) -> Res<String> {
    let ctx = LaunchCtx {
        elevator: launch::detect_elevator(),
        ..LaunchCtx::default()
    };
    let plan = lib.plan_launch(id, Some(profile), &[], &ctx).map_err(e)?;
    let mut text = plan.display();
    for n in &plan.notes {
        text.push_str(&format!("\n# note: {n}"));
    }
    Ok(text)
}

pub fn set_policy(lib: &Library, id: &str, policy: &str) -> Res<()> {
    let p = Policy::parse(policy).ok_or("policy must be auto, notify, manual or pinned")?;
    lib.set_policy(id, p).map_err(e)
}

pub fn set_source(lib: &Library, id: &str, input: &str, pattern: &str, channel: &str) -> Res<()> {
    let mut parsed = source::parse_input(input).map_err(e)?;
    if !pattern.trim().is_empty() {
        parsed.source.asset_pattern = Some(pattern.trim().to_string());
    }
    if !channel.trim().is_empty() {
        parsed.source.channel = Channel::parse(channel.trim());
    }
    lib.set_source(id, &parsed.source).map_err(e)
}

/// Run the app detached from the GUI, as the menu entry would. A thread waits
/// on it, so an app that exits never lingers as a zombie while the GUI runs.
pub fn launch_app(lib: &Library, id: &str) -> Res<()> {
    lib.get(id).map_err(e)?;
    let mut child = Command::new(&lib.exec_bin)
        .args(["run", id])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("could not start {}: {err}", lib.exec_bin.display()))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

pub fn remove_app(lib: &Library, id: &str, purge: bool) -> Res<()> {
    lib.remove(id, false).map_err(e)?;
    if purge {
        lib.purge_private_home(id).map_err(e)?;
    }
    Ok(())
}

/// Colour themes the interface offers; the first is "follow the desktop".
pub const THEMES: [&str; 3] = ["system", "gruvbox-dark", "gruvbox-light"];

/// A one-word interface setting kept in `~/.config/appofholding/<file>`, one of `choices`.
fn gui_file(dirs: &Dirs, file: &str) -> PathBuf {
    dirs.config_home.join("appofholding").join(file)
}

/// The saved choice; anything missing or unrecognised means the first one.
fn choice_load(dirs: &Dirs, file: &str, choices: &[&str]) -> String {
    std::fs::read_to_string(gui_file(dirs, file))
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| choices.contains(&t.as_str()))
        .unwrap_or_else(|| choices[0].to_string())
}

fn choice_save(dirs: &Dirs, file: &str, choices: &[&str], what: &str, name: &str) -> Res<()> {
    if !choices.contains(&name) {
        return Err(format!("unknown {what} '{name}'"));
    }
    let f = gui_file(dirs, file);
    std::fs::create_dir_all(f.parent().expect("has a parent")).map_err(e)?;
    std::fs::write(f, format!("{name}\n")).map_err(e)
}

/// The saved colour theme; anything missing or unrecognised means "system".
pub fn theme_load(dirs: &Dirs) -> String {
    choice_load(dirs, "gui-theme", &THEMES)
}

pub fn theme_save(dirs: &Dirs, name: &str) -> Res<()> {
    choice_save(dirs, "gui-theme", &THEMES, "theme", name)
}

/// Improvements worth suggesting for one app on this computer (see `core::suggest`).
pub fn suggestions(lib: &Library, id: &str) -> Res<Value> {
    let host = appofholding_core::suggest::Host::detect();
    let list = lib.suggestions(id, &host).map_err(e)?;
    Ok(Value::Array(
        list.into_iter()
            .map(|s| {
                json!({
                    "key": s.key, "title": s.title, "detail": s.detail,
                    "action": s.action, "dismissible": s.dismissible,
                })
            })
            .collect(),
    ))
}

pub fn apply_suggestion(lib: &Library, id: &str, key: &str) -> Res<Value> {
    let message = lib.apply_suggestion(id, key).map_err(e)?;
    Ok(json!({ "message": message }))
}

pub fn dismiss_suggestion(lib: &Library, id: &str, key: &str) -> Res<()> {
    lib.dismiss_suggestion(id, key).map_err(e)
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
    fn to_json(&self) -> Value {
        json!({
            "tray": self.tray,
            "minimiseToTray": self.minimise_to_tray,
            "closeToTray": self.close_to_tray,
            "notifyUpdates": self.notify_updates,
            "startInTray": self.start_in_tray,
            "dropAction": self.drop_action,
        })
    }

    fn from_json(v: &Value) -> Self {
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

fn desktop_file(dirs: &Dirs) -> PathBuf {
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

// ---- operations that need the network (the backend runs these on a worker thread)

/// What a pasted link would install: the release and which file would be picked.
pub fn resolve_link(link: &str, pattern: &str, channel: &str) -> Res<Value> {
    resolve_link_with(&UreqHttp::new(), link, pattern, channel)
}

pub fn resolve_link_with(
    http: &dyn appofholding_core::http::Http,
    link: &str,
    pattern: &str,
    channel: &str,
) -> Res<Value> {
    let mut parsed = source::parse_input(link).map_err(e)?;
    if !pattern.trim().is_empty() {
        parsed.source.asset_pattern = Some(pattern.trim().to_string());
    }
    if !channel.trim().is_empty() {
        parsed.source.channel = Channel::parse(channel.trim());
    }
    let report = appofholding_core::check::check(
        http,
        &parsed.source,
        &appofholding_core::version::Installed {
            version: None,
            sha256: None,
        },
        appofholding_core::assets::Arch::host(),
    )
    .map_err(e)?;
    let chosen = report.asset().map(|a| a.name.clone());
    let candidates: Vec<String> = match &report.selection {
        Selection::Ambiguous(v) => v.iter().map(|a| a.name.clone()).collect(),
        _ => vec![],
    };
    Ok(json!({
        "source": parsed.source.describe(),
        "release": report.latest_version(),
        "prerelease": report.release.prerelease,
        "notes": report.release.notes,
        "assets": report.release.assets.iter().map(|a| json!({"name": a.name, "size": a.size, "verified": a.sha256.is_some()})).collect::<Vec<_>>(),
        "chosen": chosen,
        "candidates": candidates,
        "problem": report.problem(),
    }))
}

pub fn install_link(
    lib: &Library,
    http: &dyn appofholding_core::http::Http,
    link: &str,
    asset_pattern: &str,
    channel: &str,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Res<Value> {
    let mut parsed = source::parse_input(link).map_err(e)?;
    if !asset_pattern.trim().is_empty() {
        parsed.source.asset_pattern = Some(asset_pattern.trim().to_string());
    }
    if !channel.trim().is_empty() {
        parsed.source.channel = Channel::parse(channel.trim());
    }
    let (rec, _) = lib
        .install_from_source(http, parsed, &AddOptions::default(), progress)
        .map_err(e)?;
    Ok(json!({"id": rec.id, "name": rec.name}))
}

// ---- importing existing AppImages

fn mode_from(text: &str) -> Res<Option<Install>> {
    Ok(match text {
        "" | "default" => None,
        "in-place" => Some(Install::InPlace),
        "copy" => Some(Install::Copy),
        "move" => Some(Install::Move),
        other => return Err(format!("unknown import mode '{other}'")),
    })
}

fn mode_name(m: Install) -> &'static str {
    match m {
        Install::InPlace => "in-place",
        Install::Copy => "copy",
        Install::Move => "move",
    }
}

fn import_plan_for(
    lib: &Library,
    home: &Path,
    opts: &Value,
) -> Res<(
    appofholding_core::import::ImportPlan,
    appofholding_core::import::Scan,
)> {
    use appofholding_core::import;
    let scan = import::scan(lib, &import::ScanOptions::new(home)).map_err(e)?;
    let plan = import::plan(
        lib,
        &scan,
        home,
        &import::PlanOptions {
            mode: mode_from(str_of(opts, "mode"))?,
            replace_entries: bool_of(opts, "replaceEntries"),
        },
    )
    .map_err(e)?;
    Ok((plan, scan))
}

/// Look for importable AppImages and say what importing each would do. Changes nothing.
pub fn import_scan(lib: &Library, home: &Path, opts: &Value) -> Res<Value> {
    use appofholding_core::import::{Action, Owner};
    let (plan, scan) = import_plan_for(lib, home, opts)?;
    let items: Vec<Value> = plan
        .items
        .iter()
        .map(|i| {
            let f = &i.found;
            let (kind, tool) = match &f.owner {
                Owner::Loose => ("loose", String::new()),
                Owner::Tool { tool, .. } => ("tool", tool.clone()),
                Owner::Entry { .. } => ("entry", String::new()),
                Owner::App { .. } => ("app", String::new()),
            };
            let (action, reason, alt) = match &i.action {
                Action::Import => ("import", String::new(), String::new()),
                Action::Conflict { reason, alt_id } => ("conflict", reason.clone(), alt_id.clone()),
                Action::Skip(why) => ("skip", why.clone(), String::new()),
            };
            json!({
                "path": f.path.display().to_string(),
                "name": f.name,
                "version": f.version,
                "size": f.size,
                "id": i.id,
                "owner": kind,
                "tool": tool,
                "action": action,
                "reason": reason,
                "altId": alt,
                "mode": mode_name(i.mode),
                "notes": i.notes,
                "replaces": i.replace_entries.len(),
                "updateSource": f.has_update_info,
            })
        })
        .collect();
    Ok(json!({
        "items": items,
        "home": home.display().to_string(),
        "searched": scan.searched.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
        "skipped": scan.skipped.iter().map(|(p, why)| json!({"path": p.display().to_string(), "reason": why})).collect::<Vec<_>>(),
    }))
}

/// Import the AppImages at `paths` (the user's final choice), planned with the same options
/// that were shown. Conflicting ones go in under their suggested alternative id.
pub fn import_run(lib: &Library, home: &Path, opts: &Value, paths: &[String]) -> Res<Value> {
    let (plan, _) = import_plan_for(lib, home, opts)?;
    let selected: Vec<usize> = plan
        .items
        .iter()
        .enumerate()
        .filter(|(_, i)| paths.iter().any(|p| Path::new(p) == i.found.path))
        .map(|(n, _)| n)
        .collect();
    if selected.is_empty() {
        return Err("nothing was selected".into());
    }
    let results = appofholding_core::import::execute(lib, &plan, &selected);
    Ok(json!({
        "results": results.iter().map(|r| match &r.outcome {
            Ok(notes) => json!({"path": r.path.display().to_string(), "id": r.id, "ok": true, "notes": notes}),
            Err(err) => json!({"path": r.path.display().to_string(), "id": r.id, "ok": false, "error": err}),
        }).collect::<Vec<_>>(),
        "imported": results.iter().filter(|r| r.outcome.is_ok()).count(),
    }))
}

/// Put back the menu entries that the last import backed up.
pub fn import_restore(lib: &Library) -> Res<Value> {
    let backups = appofholding_core::import::list_backups(lib);
    let Some(newest) = backups.first() else {
        return Ok(json!({"restored": 0}));
    };
    let restored = appofholding_core::import::restore(lib, &newest.batch).map_err(e)?;
    Ok(json!({"restored": restored.len()}))
}

fn self_json(s: &appofholding_core::selfupdate::SelfStatus) -> Value {
    use appofholding_core::selfupdate::InstallKind;
    json!({
        "current": s.current,
        "latest": s.latest,
        "available": s.update_available,
        "install": s.install.label(),
        "url": s.release_url,
        "fromCache": s.from_cache,
        // Pacman packages and AppImages are updated from inside the window.
        "canInstall": matches!(s.install, InstallKind::Pacman { .. } | InstallKind::AppImage { .. }),
        // For everything else: what to tell the user.
        "command": match s.install {
            InstallKind::Pacman { .. } => "appofholding self update --install",
            _ => "",
        },
    })
}

/// Is there a newer appOFholding release? Answers from a cache unless `force`.
pub fn self_check(
    http: &dyn appofholding_core::http::Http,
    install: appofholding_core::selfupdate::InstallKind,
    force: bool,
) -> Res<Value> {
    let dirs = Dirs::detect().map_err(e)?;
    let s = appofholding_core::selfupdate::check_cached(
        http,
        &dirs,
        install,
        appofholding_core::assets::Arch::host(),
        force,
    )
    .map_err(e)?;
    Ok(self_json(&s))
}

/// Runs a privileged command and waits for it; `Err` carries what went wrong, in words.
pub type Runner<'a> = dyn Fn(&[String]) -> Result<(), String> + 'a;

/// Run `command` (it starts with `run0`, which asks for the password in a desktop dialog).
pub fn run_privileged(command: &[String]) -> Result<(), String> {
    let out = Command::new(&command[0])
        .args(&command[1..])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|err| format!("could not start {}: {err}", command[0]))?;
    if out.status.success() {
        return Ok(());
    }
    let text =
        String::from_utf8_lossy(&out.stderr).into_owned() + &String::from_utf8_lossy(&out.stdout);
    let last = text
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .unwrap_or("it stopped with an error")
        .to_string();
    Err(last)
}

/// Update appOFholding itself. An AppImage is replaced in place; a pacman install downloads and
/// verifies the release package, then installs it with `run0 pacman -U` (the desktop asks for
/// the password; the user already confirmed in the window, so pacman's own prompt is skipped).
/// Other installs get instructions.
pub fn self_update(
    http: &dyn appofholding_core::http::Http,
    install: appofholding_core::selfupdate::InstallKind,
    run: &Runner<'_>,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Res<Value> {
    use appofholding_core::selfupdate::{self, Applied};
    let dirs = Dirs::detect().map_err(e)?;
    let status =
        selfupdate::check(http, install, appofholding_core::assets::Arch::host()).map_err(e)?;
    let work = dirs.cache_home.join("appofholding/self");
    Ok(
        match selfupdate::apply(http, &status, &work, progress).map_err(e)? {
            Applied::AlreadyCurrent => json!({"result": "current"}),
            Applied::Replaced { to, .. } => json!({"result": "replaced", "to": to}),
            Applied::PackageReady { to, command, file } => {
                // `run0 pacman -U <file>` -> also answer pacman's "Proceed?" for the user.
                let mut command = command;
                command.insert(command.len() - 1, "--noconfirm".into());
                run(&command).map_err(|why| {
                    // Dismissing the password dialog makes run0 say "Access denied" or that
                    // authentication is required; say what actually happened.
                    let lower = why.to_lowercase();
                    let why = if lower.contains("access denied")
                        || lower.contains("authentication")
                        || lower.contains("not authorized")
                    {
                        "the password was not given".to_string()
                    } else {
                        why
                    };
                    format!("appOFholding {to} was not installed: {why}. Nothing was changed.")
                })?;
                let _ = std::fs::remove_file(&file);
                json!({"result": "installed", "to": to})
            }
            Applied::Manual { to, instructions } => {
                json!({"result": "manual", "to": to, "instructions": instructions})
            }
        },
    )
}

/// A path as the desktop hands it over: a `file://` URL (percent-encoded: `My%20App.AppImage`)
/// or a plain path, which is used as it is.
pub fn local_path(s: &str) -> std::path::PathBuf {
    use std::os::unix::ffi::OsStringExt;
    let Some(rest) = s.strip_prefix("file://") else {
        return std::path::PathBuf::from(s);
    };
    // `file://host/path`: only the local host is meaningful.
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push(h * 16 + l);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    std::path::PathBuf::from(std::ffi::OsString::from_vec(out))
}

/// Add one AppImage. `mode` is `move` (into the library folder), `copy` (the default) or
/// `in-place` (leave it where it is; it stays the user's file).
pub fn add_file(lib: &Library, path: &str, mode: &str) -> Res<Value> {
    let path = local_path(path);
    let install = mode_from(mode)?.unwrap_or(Install::Copy);
    let rec = lib
        .add(&path, &AddOptions { install, id: None })
        .map_err(e)?;
    Ok(json!({"id": rec.id, "name": rec.name, "moved": install == Install::Move}))
}

/// What the window needs to know about files dropped on it before asking where they should
/// go: for each, its absolute path, its file name, and whether it already sits in the library
/// folder (then there is nothing to ask). Folders and missing files are left out.
pub fn describe_drop(lib_dir: &Path, paths: &[String]) -> Value {
    let lib_dir = std::fs::canonicalize(lib_dir).unwrap_or_else(|_| lib_dir.to_path_buf());
    Value::Array(
        paths
            .iter()
            .map(|p| local_path(p))
            .filter(|p| p.is_file())
            .map(|p| {
                let real = std::fs::canonicalize(&p).unwrap_or_else(|_| p.clone());
                json!({
                    "path": p.display().to_string(),
                    "name": p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                    "inLibrary": real.parent() == Some(lib_dir.as_path()),
                })
            })
            .collect(),
    )
}

pub fn check_app(lib: &Library, http: &dyn appofholding_core::http::Http, id: &str) -> Res<Value> {
    let r = lib.check(http, id).map_err(e)?;
    Ok(json!({"status": r.status.as_str(), "latest": r.latest_version(), "problem": r.problem()}))
}

pub fn check_all(lib: &Library, http: &(dyn appofholding_core::http::Http + Sync)) -> Res<Value> {
    let ids: Vec<String> = lib
        .sources()
        .map_err(e)?
        .into_iter()
        .map(|s| s.app_id)
        .collect();
    let results = lib
        .check_many(http, &ids)
        .into_iter()
        .map(|(id, r)| match r {
            Ok(r) => {
                json!({"id": id, "status": r.status.as_str(), "latest": r.latest_version(), "problem": r.problem()})
            }
            Err(err) => json!({"id": id, "error": err.to_string()}),
        })
        .collect();
    Ok(Value::Array(results))
}

/// The tray's background check: check every app, then hand out the updates the user has not
/// been told about yet ({results, fresh: [{id, name, version}]}).
pub fn notify_check(
    lib: &Library,
    http: &(dyn appofholding_core::http::Http + Sync),
) -> Res<Value> {
    let results = check_all(lib, http)?;
    let fresh = lib
        .take_unannounced_updates()
        .map_err(e)?
        .into_iter()
        .map(|u| json!({"id": u.id, "name": u.name, "version": u.version}))
        .collect::<Vec<_>>();
    Ok(json!({"results": results, "fresh": fresh}))
}

pub fn update_app(
    lib: &Library,
    http: &dyn appofholding_core::http::Http,
    id: &str,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Res<Value> {
    let opts = ApplyOptions {
        if_running: IfRunning::Proceed,
        ..ApplyOptions::default()
    };
    let applied = lib.update(http, id, &opts, progress).map_err(e)?;
    let warnings = lib.take_warnings();
    Ok(match applied {
        Applied::Updated { from, to, delta } => {
            json!({"result": "updated", "from": from, "to": to, "delta": delta, "warnings": warnings})
        }
        Applied::NoUpdate(s) => json!({"result": "none", "status": s.as_str()}),
        Applied::Skipped(why) => json!({"result": "skipped", "why": why}),
        Applied::Deferred { version, .. } => json!({"result": "deferred", "version": version}),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use appofholding_core::fixture;
    use appofholding_core::http::fake::FakeHttp;

    fn lib() -> (tempfile::TempDir, Library) {
        let tmp = tempfile::tempdir().unwrap();
        let lib =
            Library::open(Dirs::with_root(tmp.path()), tmp.path().join("appofholding")).unwrap();
        (tmp, lib)
    }

    #[test]
    fn the_theme_choice_is_saved_and_validated() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::with_root(tmp.path());
        assert_eq!(theme_load(&dirs), "system");
        theme_save(&dirs, "gruvbox-light").unwrap();
        assert_eq!(theme_load(&dirs), "gruvbox-light");
        assert!(theme_save(&dirs, "../evil").is_err());
        std::fs::write(gui_file(&dirs, "gui-theme"), "garbage").unwrap();
        assert_eq!(theme_load(&dirs), "system");
    }

    /// Serialises the tests that point `APPOFHOLDING_ROOT` somewhere (it is process-wide).
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn a_pacman_install_updates_from_the_window_through_run0() {
        use appofholding_core::selfupdate::InstallKind;
        use sha2::{Digest, Sha256};
        let pkg = b"fake package".to_vec();
        let api = "https://api.github.com/repos/asafelobotomy/appofholding/releases/latest";
        let release = format!(
            r#"{{"tag_name":"v99.0.0","prerelease":false,"draft":false,"assets":[
              {{"name":"appofholding-99.0.0-1-x86_64.pkg.tar.zst","browser_download_url":"https://dl/pkg","size":{},"digest":"sha256:{}"}}]}}"#,
            pkg.len(),
            hex::encode(Sha256::digest(&pkg))
        );
        let http = FakeHttp::default()
            .json(api, &release)
            .bytes("https://dl/pkg", pkg);
        let tmp = tempfile::tempdir().unwrap();
        let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        // SAFETY: tests that set this variable hold ENV_LOCK.
        unsafe { std::env::set_var("APPOFHOLDING_ROOT", tmp.path()) };
        let pacman = || InstallKind::Pacman {
            package: "appofholding".into(),
        };

        let seen = std::cell::RefCell::new(Vec::new());
        let ok = |c: &[String]| -> Result<(), String> {
            seen.borrow_mut().push(c.to_vec());
            Ok(())
        };
        let r = self_update(&http, pacman(), &ok, &mut |_, _| {}).unwrap();
        assert_eq!(
            (r["result"].clone(), r["to"].clone()),
            (json!("installed"), json!("99.0.0"))
        );
        let cmd = &seen.borrow()[0];
        assert_eq!(&cmd[..4], ["run0", "pacman", "-U", "--noconfirm"]);
        assert!(
            cmd[4].ends_with("appofholding-99.0.0-1-x86_64.pkg.tar.zst"),
            "{cmd:?}"
        );

        // Cancelling the password dialog (or any pacman error) is reported, not hidden.
        let refused = |_: &[String]| -> Result<(), String> {
            Err("Failed to start transient service unit: Access denied".into())
        };
        let err = self_update(&http, pacman(), &refused, &mut |_, _| {}).unwrap_err();
        assert_eq!(
            err,
            "appOFholding 99.0.0 was not installed: the password was not given. Nothing was changed."
        );
        let broken = |_: &[String]| -> Result<(), String> {
            Err("error: failed to commit transaction".into())
        };
        let err = self_update(&http, pacman(), &broken, &mut |_, _| {}).unwrap_err();
        assert!(err.contains("failed to commit transaction"), "{err}");
    }

    #[test]
    fn desktop_file_urls_become_real_paths() {
        assert_eq!(
            local_path("file:///home/u/My%20App%E2%9C%93.AppImage"),
            Path::new("/home/u/My App✓.AppImage")
        );
        assert_eq!(
            local_path("file://localhost/tmp/a%25b"),
            Path::new("/tmp/a%b")
        );
        // Plain paths are taken literally, even with a % in them.
        assert_eq!(local_path("/tmp/100%20done"), Path::new("/tmp/100%20done"));
        // A broken escape is kept as written.
        assert_eq!(local_path("file:///tmp/50%"), Path::new("/tmp/50%"));
    }

    #[test]
    fn suggestions_are_listed_applied_and_dismissed_through_the_service() {
        let tmp = tempfile::tempdir().unwrap();
        let lib =
            Library::open(Dirs::with_root(tmp.path()), tmp.path().join("appofholding")).unwrap();
        add_foo(tmp.path(), &lib);
        // A plain app: nothing to suggest, and the list is a JSON array.
        assert_eq!(suggestions(&lib, "foo-app").unwrap(), json!([]));
        assert!(apply_suggestion(&lib, "foo-app", "nonsense").is_err());
        dismiss_suggestion(&lib, "foo-app", "electron-wayland").unwrap();
        assert!(suggestions(&lib, "missing").is_err());
    }

    #[test]
    fn dropped_files_are_described_and_added_by_mode() {
        let (tmp, lib) = lib();
        let elsewhere = tmp.path().join("Downloads");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let mk = |dir: &Path, file: &str, name: &str| {
            let p = dir.join(file);
            std::fs::write(&p, fixture::simple_appimage(name, None)).unwrap();
            p.display().to_string()
        };
        let moved = mk(&elsewhere, "Moved.AppImage", "Moved App");
        let copied = mk(&elsewhere, "Copied.AppImage", "Copied App");
        let kept = mk(&elsewhere, "Kept.AppImage", "Kept App");
        std::fs::create_dir_all(&lib.dirs.library_dir).unwrap();
        let already = mk(&lib.dirs.library_dir, "Already.AppImage", "Already App");

        let d = describe_drop(
            &lib.dirs.library_dir,
            &[
                format!("file://{moved}"),
                already.clone(),
                elsewhere.display().to_string(), // a folder: left out
                "/nonexistent/x.AppImage".into(),
            ],
        );
        let d = d.as_array().unwrap();
        assert_eq!(d.len(), 2, "{d:?}");
        assert_eq!(d[0]["name"], "Moved.AppImage");
        assert_eq!(d[0]["inLibrary"], false);
        assert_eq!(d[1]["inLibrary"], true);

        let r = add_file(&lib, &moved, "move").unwrap();
        assert_eq!(r["moved"], true);
        assert!(!Path::new(&moved).exists(), "moved away");
        assert!(lib.dirs.library_dir.join("Moved.AppImage").exists());

        add_file(&lib, &copied, "copy").unwrap();
        assert!(
            Path::new(&copied).exists() && lib.dirs.library_dir.join("Copied.AppImage").exists()
        );

        let r = add_file(&lib, &kept, "in-place").unwrap();
        let rec = lib.get(r["id"].as_str().unwrap()).unwrap();
        assert!(rec.external_file && rec.appimage_path == std::fs::canonicalize(&kept).unwrap());

        add_file(&lib, &already, "move").unwrap();
        assert!(
            Path::new(&already).exists(),
            "already in the library: stays put"
        );

        assert!(add_file(&lib, &kept, "sideways").is_err());
    }

    #[test]
    fn desktop_settings_and_autostart() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::with_root(tmp.path());
        let d = desktop_load(&dirs);
        assert_eq!(d["tray"], false);
        assert_eq!(d["notifyUpdates"], true);
        assert_eq!(d["autostart"], false);
        assert_eq!(d["dropAction"], "ask");

        let exe = Path::new("/opt/My Apps/appofholding-gui");
        let on = json!({"tray": true, "closeToTray": true, "startInTray": true, "autostart": true});
        desktop_save(&dirs, &on, exe).unwrap();
        let d = desktop_load(&dirs);
        assert_eq!(d["tray"], true);
        assert_eq!(d["closeToTray"], true);
        assert_eq!(d["minimiseToTray"], false);
        assert_eq!(d["autostart"], true);
        let entry = std::fs::read_to_string(autostart_file(&dirs)).unwrap();
        assert!(
            entry.contains("Exec=\"/opt/My Apps/appofholding-gui\" --tray\n"),
            "{entry}"
        );

        // Without a tray icon it cannot start hidden: the window opens.
        desktop_save(&dirs, &json!({"tray": false, "autostart": true}), exe).unwrap();
        let entry = std::fs::read_to_string(autostart_file(&dirs)).unwrap();
        assert!(!entry.contains("--tray"), "{entry}");

        desktop_save(&dirs, &json!({"autostart": false}), exe).unwrap();
        assert!(!autostart_file(&dirs).exists());
        assert_eq!(desktop_load(&dirs)["autostart"], false);
        // Turning it off twice is fine; a relative path is refused.
        desktop_save(&dirs, &json!({"autostart": false}), exe).unwrap();
        assert!(desktop_save(&dirs, &json!({"autostart": true}), Path::new("gui")).is_err());

        desktop_save(&dirs, &json!({"dropAction": "move"}), exe).unwrap();
        assert_eq!(desktop_load(&dirs)["dropAction"], "move");
        desktop_save(&dirs, &json!({"dropAction": "shred"}), exe).unwrap();
        assert_eq!(desktop_load(&dirs)["dropAction"], "ask");

        std::fs::write(desktop_file(&dirs), "garbage").unwrap();
        assert_eq!(desktop_load(&dirs)["tray"], false);
    }

    #[test]
    fn the_library_view_is_saved_and_validated() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::with_root(tmp.path());
        assert_eq!(view_load(&dirs), "list");
        view_save(&dirs, "grid").unwrap();
        assert_eq!(view_load(&dirs), "grid");
        view_save(&dirs, "compact").unwrap();
        assert_eq!(view_load(&dirs), "compact");
        assert!(view_save(&dirs, "../evil").is_err());
        std::fs::write(gui_file(&dirs, "gui-view"), "garbage").unwrap();
        assert_eq!(view_load(&dirs), "list");
    }

    fn add_foo(tmp: &Path, lib: &Library) {
        let p = tmp.join("Foo.AppImage");
        std::fs::write(
            &p,
            fixture::simple_appimage(
                "Foo App",
                Some("gh-releases-zsync|o|foo|latest|Foo-*-x86_64.AppImage.zsync"),
            ),
        )
        .unwrap();
        lib.add(&p, &AddOptions::default()).unwrap();
    }

    #[test]
    fn list_and_detail_describe_an_app_for_the_interface() {
        let (tmp, lib) = lib();
        assert_eq!(list_apps(&lib).unwrap(), json!([]));
        add_foo(tmp.path(), &lib);
        let list = list_apps(&lib).unwrap();
        let a = &list[0];
        assert_eq!(
            (a["id"].as_str(), a["name"].as_str(), a["version"].as_str()),
            (Some("foo-app"), Some("Foo App"), Some("1.2.3"))
        );
        assert!(
            a["icon"].as_str().unwrap().starts_with("file://") && a["hasSource"] == json!(true)
        );
        assert_eq!(a["sandbox"], "none");

        let d = app_detail(&lib, "foo-app").unwrap();
        assert_eq!(d["source"]["origin"], "embedded");
        assert_eq!(d["source"]["policy"], "notify");
        assert_eq!(d["profiles"], json!(["default"]));
        assert_eq!(d["sandbox"]["level"], "none");
        assert_eq!(d["running"], json!(false));
        assert!(app_detail(&lib, "nope").is_err());
    }

    #[test]
    fn launch_profile_round_trips_through_the_form_representation() {
        let (tmp, lib) = lib();
        add_foo(tmp.path(), &lib);
        let form = json!({
            "args": "--hello \"two words\"", "env": "A=1\nB=x y\n\n", "prefix": "nice -n 5",
            "cwd": "/srv", "presets": ["x11", "debug"], "extractAndRun": true, "elevated": false
        });
        save_profile(&lib, "foo-app", "default", &form).unwrap();
        let back = profile_detail(&lib, "foo-app", "default").unwrap();
        assert_eq!(
            back["args"],
            "--hello 'two words'".replace('\'', "\"").as_str()
        );
        assert_eq!(back["env"], "A=1\nB=x y");
        assert_eq!(back["prefix"], "nice -n 5");
        assert_eq!(back["presets"], json!(["x11", "debug"]));
        assert_eq!(back["extractAndRun"], json!(true));

        assert!(
            save_profile(&lib, "foo-app", "default", &json!({"env": "NOEQUALS"}))
                .unwrap_err()
                .contains("NAME=value")
        );
        assert!(
            save_profile(&lib, "foo-app", "default", &json!({"presets": ["nope"]}))
                .unwrap_err()
                .contains("unknown preset")
        );
        // An extra profile shows up in the detail's profile list and can be removed again.
        save_profile(&lib, "foo-app", "debug", &json!({"presets": ["debug"]})).unwrap();
        assert_eq!(
            app_detail(&lib, "foo-app").unwrap()["profiles"],
            json!(["default", "debug"])
        );
        delete_profile(&lib, "foo-app", "debug").unwrap();
        assert_eq!(
            app_detail(&lib, "foo-app").unwrap()["profiles"],
            json!(["default"])
        );
    }

    #[test]
    fn sandbox_form_saves_normalises_and_validates() {
        let (tmp, lib) = lib();
        add_foo(tmp.path(), &lib);
        let strict = sandbox_defaults("strict").unwrap();
        assert_eq!(
            (strict["network"].clone(), strict["bus"].clone()),
            (json!(false), json!("off"))
        );
        let saved = save_sandbox(&lib, "foo-app", &strict).unwrap();
        assert_eq!(saved["level"], "strict");
        // Flip one switch: it is no longer the strict preset.
        let mut tweaked = strict.clone();
        tweaked["network"] = json!(true);
        tweaked["paths"] = json!([{"path": "/tmp", "write": true}]);
        let saved = save_sandbox(&lib, "foo-app", &tweaked).unwrap();
        assert_eq!(saved["level"], "custom");
        assert_eq!(saved["paths"], json!([{"path": "/tmp", "write": true}]));
        assert!(save_sandbox(&lib, "foo-app", &json!({"level": "paranoid"})).is_err());
        assert!(
            save_sandbox(
                &lib,
                "foo-app",
                &json!({"level": "standard", "home": "x", "bus": "off"})
            )
            .is_err()
        );
        let bad = json!({"level": "none", "paths": [{"path": "/proc", "write": false}]});
        assert!(
            save_sandbox(&lib, "foo-app", &bad)
                .unwrap_err()
                .contains("cannot be shared")
        );
        // Switching it off clears the effect.
        assert_eq!(
            save_sandbox(&lib, "foo-app", &json!({"level": "none"})).unwrap()["level"],
            "none"
        );
    }

    #[test]
    fn preview_shows_the_command_or_the_reason_there_is_none() {
        let (tmp, lib) = lib();
        add_foo(tmp.path(), &lib);
        let p = preview(&lib, "foo-app", "default").unwrap();
        assert!(p.contains("Foo.AppImage"), "{p}");
        assert!(
            preview(&lib, "foo-app", "ghost")
                .unwrap_err()
                .contains("no launch profile")
        );
    }

    #[test]
    fn policy_and_source_forms() {
        let (tmp, lib) = lib();
        add_foo(tmp.path(), &lib);
        set_policy(&lib, "foo-app", "auto").unwrap();
        assert!(set_policy(&lib, "foo-app", "sometimes").is_err());
        set_source(
            &lib,
            "foo-app",
            "https://github.com/o/other/",
            "Other-*.AppImage",
            "latest-pre",
        )
        .unwrap();
        let d = app_detail(&lib, "foo-app").unwrap();
        assert_eq!(d["source"]["describe"], "github.com/o/other [latest-pre]");
        assert_eq!(d["source"]["pattern"], "Other-*.AppImage");
        assert_eq!(d["source"]["policy"], "auto");
        assert!(set_source(&lib, "foo-app", "ftp://x", "", "").is_err());
    }

    const REL: &str = r#"{"tag_name":"v2.0.0","prerelease":false,"draft":false,"body":"Notes","assets":[
        {"name":"Foo-2.0.0-x86_64.AppImage","browser_download_url":"https://dl/a","digest":"sha256:aa"},
        {"name":"Foo-2.0.0-x86_64.AppImage.zsync","browser_download_url":"https://dl/z"}]}"#;

    #[test]
    fn resolving_a_link_previews_the_release_and_the_chosen_file() {
        let http =
            FakeHttp::default().json("https://api.github.com/repos/o/foo/releases/latest", REL);
        let v = resolve_link_with(&http, "https://github.com/o/foo/", "", "").unwrap();
        assert_eq!(v["release"], "v2.0.0");
        assert_eq!(v["chosen"], "Foo-2.0.0-x86_64.AppImage");
        assert_eq!(v["assets"][0]["verified"], json!(true));
        assert_eq!(v["problem"], Value::Null);
        // A pattern that matches nothing explains itself instead of failing.
        let v = resolve_link_with(&http, "o/foo", "Nope-*.AppImage", "").unwrap();
        assert_eq!(v["chosen"], Value::Null);
        assert!(
            v["problem"]
                .as_str()
                .unwrap()
                .contains("no AppImage matching")
        );
        assert!(resolve_link_with(&http, "not a link at all", "", "").is_err());
    }

    #[test]
    fn install_check_and_update_flow_with_progress() {
        let (_tmp, lib) = lib();
        let img = fixture::versioned_appimage(
            "Foo App",
            "1.0.0",
            Some("gh-releases-zsync|o|foo|latest|Foo-*-x86_64.AppImage.zsync"),
        );
        let sha = hex_sha256(&img);
        let rel = |ver: &str, img: &[u8], sha: &str| {
            format!(
                r#"{{"tag_name":"v{ver}","prerelease":false,"draft":false,"assets":[{{"name":"Foo-{ver}-x86_64.AppImage","browser_download_url":"https://dl/{ver}","size":{},"digest":"sha256:{sha}"}}]}}"#,
                img.len()
            )
        };
        let http = FakeHttp::default()
            .json(
                "https://api.github.com/repos/o/foo/releases/latest",
                &rel("1.0.0", &img, &sha),
            )
            .bytes("https://dl/1.0.0", img.clone());
        let mut seen = 0u32;
        let v = install_link(&lib, &http, "o/foo", "", "", &mut |_, _| seen += 1).unwrap();
        assert_eq!(v["id"], "foo-app");
        assert!(seen > 0, "progress was reported");
        assert_eq!(
            app_detail(&lib, "foo-app").unwrap()["source"]["status"],
            "up-to-date"
        );

        let img2 = fixture::versioned_appimage(
            "Foo App",
            "2.0.0",
            Some("gh-releases-zsync|o|foo|latest|Foo-*-x86_64.AppImage.zsync"),
        );
        let sha2 = hex_sha256(&img2);
        let http2 = FakeHttp::default()
            .json(
                "https://api.github.com/repos/o/foo/releases/latest",
                &rel("2.0.0", &img2, &sha2),
            )
            .bytes("https://dl/2.0.0", img2);
        let c = check_app(&lib, &http2, "foo-app").unwrap();
        assert_eq!(
            (c["status"].clone(), c["latest"].clone()),
            (json!("update-available"), json!("v2.0.0"))
        );
        let all = check_all(&lib, &http2).unwrap();
        assert_eq!(all[0]["status"], "update-available");
        assert_eq!(list_apps(&lib).unwrap()[0]["status"], "update-available");

        let u = update_app(&lib, &http2, "foo-app", &mut |_, _| {}).unwrap();
        assert_eq!(
            (u["result"].clone(), u["to"].clone()),
            (json!("updated"), json!("v2.0.0"))
        );
        assert_eq!(
            app_detail(&lib, "foo-app").unwrap()["versions"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let again = update_app(&lib, &http2, "foo-app", &mut |_, _| {}).unwrap();
        assert_eq!(again["result"], "none");
        // Errors come back as readable strings, not panics.
        assert!(check_app(&lib, &FakeHttp::default(), "foo-app").is_err());
    }

    fn hex_sha256(b: &[u8]) -> String {
        use std::process::Stdio;
        // Avoid another dependency: use the core library's hashing through a temp file.
        let f = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(f.path(), b).unwrap();
        let _ = Stdio::null();
        appofholding_core::image::hash_file(f.path()).unwrap().0
    }

    #[test]
    fn import_dialog_flow_scan_then_confirm() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir_all(home.join("AppImages")).unwrap();
        std::fs::write(
            home.join("AppImages/Loose.AppImage"),
            fixture::simple_appimage("Loose App", None),
        )
        .unwrap();
        let gear = home.join("AppImages/gear.appimage");
        std::fs::write(&gear, fixture::simple_appimage("Gear App", None)).unwrap();
        let lib = Library::open(
            Dirs::with_root(&tmp.path().join("lib")),
            tmp.path().join("appofholding"),
        )
        .unwrap();
        std::fs::create_dir_all(lib.dirs.applications_dir()).unwrap();
        std::fs::write(
            lib.dirs.applications_dir().join("gear.desktop"),
            format!("[Desktop Entry]\nType=Application\nName=Gear App\nExec=env DESKTOPINTEGRATION=1 {0}\nTryExec={0}\n", gear.display()),
        )
        .unwrap();

        let opts = json!({"mode": "default", "replaceEntries": false});
        let v = import_scan(&lib, &home, &opts).unwrap();
        let items = v["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        let g = items.iter().find(|i| i["name"] == "Gear App").unwrap();
        assert_eq!(
            (
                g["owner"].clone(),
                g["tool"].clone(),
                g["action"].clone(),
                g["mode"].clone()
            ),
            (
                json!("tool"),
                json!("Gear Lever"),
                json!("import"),
                json!("in-place")
            )
        );
        assert!(
            g["notes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|n| n.as_str().unwrap().contains("appear twice"))
        );
        assert!(lib.list().unwrap().is_empty(), "scanning is a dry run");

        // Changing the options changes the shown plan.
        let with = import_scan(
            &lib,
            &home,
            &json!({"mode": "copy", "replaceEntries": true}),
        )
        .unwrap();
        let g = with["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["name"] == "Gear App")
            .unwrap()
            .clone();
        assert_eq!(
            (g["mode"].clone(), g["replaces"].clone()),
            (json!("copy"), json!(1))
        );
        assert!(import_scan(&lib, &home, &json!({"mode": "teleport"})).is_err());

        // The final confirmation imports exactly what was chosen.
        let r = import_run(&lib, &home, &opts, &[gear.display().to_string()]).unwrap();
        assert_eq!(r["imported"], 1);
        assert_eq!(
            lib.list()
                .unwrap()
                .iter()
                .map(|a| a.id.clone())
                .collect::<Vec<_>>(),
            vec!["gear-app".to_string()]
        );
        assert!(
            import_run(&lib, &home, &opts, &["/nonexistent".to_string()])
                .unwrap_err()
                .contains("nothing was selected")
        );
        assert_eq!(import_restore(&lib).unwrap()["restored"], 0);
    }

    #[test]
    fn self_update_json_for_the_banner() {
        use appofholding_core::selfupdate::InstallKind;
        let api = "https://api.github.com/repos/asafelobotomy/appofholding/releases/latest";
        let http = FakeHttp::default().json(
            api,
            r#"{"tag_name":"v99.0.0","prerelease":false,"draft":false,"assets":[]}"#,
        );
        let tmp = tempfile::tempdir().unwrap();
        let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        // SAFETY: tests that set this variable hold ENV_LOCK.
        unsafe { std::env::set_var("APPOFHOLDING_ROOT", tmp.path()) };
        let v = self_check(
            &http,
            InstallKind::Pacman {
                package: "appofholding".into(),
            },
            true,
        )
        .unwrap();
        assert_eq!(
            (
                v["available"].clone(),
                v["latest"].clone(),
                v["install"].clone()
            ),
            (json!(true), json!("99.0.0"), json!("pacman"))
        );
        assert_eq!(v["command"], "appofholding self update --install");
        assert!(
            v["url"]
                .as_str()
                .unwrap()
                .ends_with("/releases/tag/v99.0.0")
        );
        // The second answer comes from the cache and still says an update exists.
        let again = self_check(&http, InstallKind::AppImage { path: "/x".into() }, false).unwrap();
        assert_eq!(
            (
                again["fromCache"].clone(),
                again["available"].clone(),
                again["command"].clone()
            ),
            (json!(true), json!(true), json!(""))
        );
        assert_eq!(v["canInstall"], json!(true));
        // An install we cannot update ourselves gets guidance, not a half-finished update.
        let never = |_: &[String]| -> Result<(), String> { panic!("nothing may run") };
        let r = self_update(&http, InstallKind::Unknown, &never, &mut |_, _| {}).unwrap();
        assert_eq!(r["result"], "manual");
        assert!(
            r["instructions"]
                .as_str()
                .unwrap()
                .contains("releases/tag/v99.0.0")
        );
        unsafe { std::env::remove_var("APPOFHOLDING_ROOT") };
    }

    #[test]
    fn add_remove_and_schedule() {
        let (tmp, lib) = lib();
        let p = tmp.path().join("Foo.AppImage");
        std::fs::write(&p, fixture::simple_appimage("Foo App", None)).unwrap();
        let v = add_file(&lib, &format!("file://{}", p.display()), "").unwrap();
        assert_eq!(v["id"], "foo-app");
        assert!(
            add_file(&lib, p.to_str().unwrap(), "").is_err(),
            "adding twice is reported"
        );
        remove_app(&lib, "foo-app", true).unwrap();
        assert_eq!(list_apps(&lib).unwrap(), json!([]));

        let dirs = Dirs::with_root(tmp.path());
        assert_eq!(schedule_status(&dirs)["installed"], json!(false));
        schedule_install(&dirs, "weekly").unwrap();
        assert_eq!(schedule_status(&dirs)["installed"], json!(true));
        assert!(schedule_install(&dirs, "yearly").is_err());
        schedule_remove(&dirs).unwrap();
        assert_eq!(schedule_status(&dirs)["installed"], json!(false));
    }
}
