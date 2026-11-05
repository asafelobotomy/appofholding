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

/// Run the app detached from the GUI, as the menu entry would.
pub fn launch_app(lib: &Library, id: &str) -> Res<()> {
    lib.get(id).map_err(e)?;
    Command::new(&lib.exec_bin)
        .args(["run", id])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("could not start {}: {err}", lib.exec_bin.display()))
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

fn theme_file(dirs: &Dirs) -> std::path::PathBuf {
    dirs.config_home.join("appofholding").join("gui-theme")
}

/// The saved colour theme; anything missing or unrecognised means "system".
pub fn theme_load(dirs: &Dirs) -> String {
    std::fs::read_to_string(theme_file(dirs))
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| THEMES.contains(&t.as_str()))
        .unwrap_or_else(|| THEMES[0].to_string())
}

pub fn theme_save(dirs: &Dirs, name: &str) -> Res<()> {
    if !THEMES.contains(&name) {
        return Err(format!("unknown theme '{name}'"));
    }
    let f = theme_file(dirs);
    std::fs::create_dir_all(f.parent().expect("has a parent")).map_err(e)?;
    std::fs::write(f, format!("{name}\n")).map_err(e)
}

pub fn schedule_status(dirs: &Dirs) -> Value {
    let (svc, timer) = schedule::unit_paths(dirs);
    json!({ "installed": svc.exists() && timer.exists() })
}

pub fn schedule_install(dirs: &Dirs, interval: &str) -> Res<()> {
    let iv = Interval::parse(interval).ok_or("interval must be hourly, daily or weekly")?;
    schedule::write_units(dirs, &cli_path(), iv).map_err(e)?;
    if dirs.refresh_caches {
        let ok = |args: &[&str]| {
            Command::new("systemctl")
                .arg("--user")
                .args(args)
                .status()
                .is_ok_and(|s| s.success())
        };
        if !(ok(&["daemon-reload"]) && ok(&["enable", "--now", "appofholding-update.timer"])) {
            return Err("wrote the timer files but systemd would not enable them".into());
        }
    }
    Ok(())
}

pub fn schedule_remove(dirs: &Dirs) -> Res<()> {
    if dirs.refresh_caches {
        let _ = Command::new("systemctl")
            .args(["--user", "disable", "--now", "appofholding-update.timer"])
            .status();
    }
    schedule::remove_units(dirs).map(|_| ()).map_err(e)?;
    if dirs.refresh_caches {
        let _ = Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status();
    }
    Ok(())
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
        // What to tell the user when the update cannot be done from inside the window.
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

/// Update appOFholding itself where it can be done without a password prompt (AppImage
/// installs); other installs get the command to run.
pub fn self_update(
    http: &dyn appofholding_core::http::Http,
    install: appofholding_core::selfupdate::InstallKind,
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
            Applied::PackageReady { to, command, .. } => {
                json!({"result": "package", "to": to, "command": command.join(" ")})
            }
            Applied::Manual { to, instructions } => {
                json!({"result": "manual", "to": to, "instructions": instructions})
            }
        },
    )
}

pub fn add_file(lib: &Library, path: &str) -> Res<Value> {
    let path = path.strip_prefix("file://").unwrap_or(path);
    let rec = lib
        .add(
            Path::new(path),
            &AddOptions {
                install: Install::Copy,
                id: None,
            },
        )
        .map_err(e)?;
    Ok(json!({"id": rec.id, "name": rec.name}))
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
        std::fs::write(theme_file(&dirs), "garbage").unwrap();
        assert_eq!(theme_load(&dirs), "system");
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
        // SAFETY: only this test in this binary sets the variable.
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
        // A package install gets guidance, not a half-finished update.
        let r = self_update(&http, InstallKind::Unknown, &mut |_, _| {}).unwrap();
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
        let v = add_file(&lib, &format!("file://{}", p.display())).unwrap();
        assert_eq!(v["id"], "foo-app");
        assert!(
            add_file(&lib, p.to_str().unwrap()).is_err(),
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
