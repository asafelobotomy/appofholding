//! What uQ's Settings page (and any other program) reads and writes through the command line:
//! `config`, the `--dry-run --json` plans of `remove` and `update`, and the newline-delimited
//! progress events of `check --ndjson` and `update --ndjson`.
//!
//! Every `--json` answer is one object with `"version": 1`. A failure prints
//! `{"version": 1, "ok": false, "error": "..."}` and exits 1, so a caller that parses output
//! whatever the exit code still gets the reason.

use std::fs;
use std::path::Path;

use anyhow::{Result, anyhow, bail};
use appofholding_core::defaults::MAX_KEEP_VERSIONS;
use appofholding_core::engine::{self, DEFAULT_KEEP_VERSIONS};
use appofholding_core::paths::stable_exec_path;
use appofholding_core::sandbox::Level;
use appofholding_core::scan::ScanMode;
use appofholding_core::schedule::{self, Interval};
use appofholding_core::source::Policy;
use appofholding_core::{AppRecord, Dirs, FilePolicy, Library, SourceRecord};
use clap::Subcommand;
use serde_json::{Value, json};

/// The settings `config` shows and changes.
pub const KEYS: [&str; 5] = ["policy", "keep_versions", "sandbox", "schedule", "scan"];

#[derive(Subcommand)]
pub enum ConfigCmd {
    /// Show every setting: the update policy new sources start with, how many previous versions
    /// an update keeps, the sandbox new apps start in, the background check schedule, and whether
    /// oysterAV scans AppImages.
    Show {
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
    /// Change one setting: policy (auto, notify, manual, pinned), keep_versions (0-10),
    /// sandbox (none, standard, strict), schedule (off, hourly, daily, weekly) or scan (off, warn,
    /// block: whether oysterAV scans an AppImage as it is added or updated, and whether a threat
    /// stops it).
    Set {
        key: String,
        value: String,
        /// With `policy`: also give every app that has an update source this policy.
        #[arg(long)]
        all: bool,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
    /// Go back to the built-in value of a setting (the schedule goes off).
    Reset {
        key: String,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
}

impl ConfigCmd {
    pub fn json(&self) -> bool {
        match self {
            ConfigCmd::Show { json }
            | ConfigCmd::Set { json, .. }
            | ConfigCmd::Reset { json, .. } => *json,
        }
    }
}

/// Prints the error envelope for a failed `--json` command.
pub fn print_error(e: &anyhow::Error) {
    println!(
        "{}",
        json!({ "version": 1, "ok": false, "error": format!("{e:#}") })
    );
}

fn schedule_name(dirs: &Dirs) -> &'static str {
    if !schedule::installed(dirs) {
        return "off";
    }
    // A timer edited to another calendar still counts as on; "daily" is the nearest name.
    schedule::installed_interval(dirs).map_or("daily", Interval::as_str)
}

fn settings_json(lib: &Library) -> Result<Value> {
    let d = lib.defaults()?;
    Ok(json!({
        "policy": d.policy.as_str(),
        "keep_versions": d.keep_versions,
        "sandbox": d.sandbox.as_str(),
        "schedule": schedule_name(&lib.dirs),
        "scan": lib.scan_mode()?.as_str(),
    }))
}

pub fn show_json(lib: &Library) -> Result<Value> {
    Ok(json!({
        "version": 1,
        "ok": true,
        "settings": settings_json(lib)?,
        "defaults": {
            "policy": Policy::Notify.as_str(),
            "keep_versions": DEFAULT_KEEP_VERSIONS,
            "sandbox": Level::None.as_str(),
            "schedule": "off",
            "scan": "off",
        },
        "limits": { "max_keep_versions": MAX_KEEP_VERSIONS },
        "apps_with_source": lib.sources()?.len(),
    }))
}

pub fn show_text(lib: &Library) -> Result<()> {
    let v = settings_json(lib)?;
    for k in KEYS {
        println!("{k:<14} {}", v[k].to_string().trim_matches('"'));
    }
    Ok(())
}

/// Checks every value before changing anything, then writes it. Returns what was done.
pub fn set(lib: &Library, exe: &Path, key: &str, value: &str, all: bool) -> Result<Value> {
    if all && key != "policy" {
        bail!("--all only goes with the policy setting");
    }
    let mut changed_apps = 0;
    match key {
        "policy" => {
            let p = Policy::parse(value)
                .ok_or_else(|| anyhow!("policy must be auto, notify, manual or pinned"))?;
            lib.set_default_policy(p)?;
            if all {
                changed_apps = lib.set_policy_all(p)?;
            }
        }
        "keep_versions" => {
            let n: usize = value.parse().map_err(|_| {
                anyhow!("keep_versions must be a whole number from 0 to {MAX_KEEP_VERSIONS}")
            })?;
            if n > MAX_KEEP_VERSIONS {
                bail!("at most {MAX_KEEP_VERSIONS} previous versions can be kept");
            }
            lib.set_keep_versions(n)?;
        }
        "sandbox" => match Level::parse(value) {
            Some(l @ (Level::None | Level::Standard | Level::Strict)) => {
                lib.set_default_sandbox(l)?
            }
            _ => bail!("sandbox must be none, standard or strict"),
        },
        "schedule" => set_schedule(&lib.dirs, exe, value)?,
        "scan" => {
            let mode =
                ScanMode::parse(value).ok_or_else(|| anyhow!("scan must be off, warn or block"))?;
            lib.set_scan_mode(mode)?;
        }
        _ => bail!(
            "unknown setting '{key}'; the settings are {}",
            KEYS.join(", ")
        ),
    }
    Ok(json!({
        "version": 1,
        "ok": true,
        "key": key,
        "settings": settings_json(lib)?,
        "changed_apps": changed_apps,
    }))
}

pub fn reset(lib: &Library, exe: &Path, key: &str) -> Result<Value> {
    let value = match key {
        "policy" => Policy::Notify.as_str().to_string(),
        "keep_versions" => DEFAULT_KEEP_VERSIONS.to_string(),
        "sandbox" => Level::None.as_str().to_string(),
        "schedule" => "off".to_string(),
        "scan" => "off".to_string(),
        _ => bail!(
            "unknown setting '{key}'; the settings are {}",
            KEYS.join(", ")
        ),
    };
    set(lib, exe, key, &value, false)
}

fn set_schedule(dirs: &Dirs, exe: &Path, value: &str) -> Result<()> {
    if value == "off" {
        schedule::disable(dirs)?;
        return Ok(());
    }
    let iv = Interval::parse(value)
        .ok_or_else(|| anyhow!("schedule must be off, hourly, daily or weekly"))?;
    if !schedule::enable(dirs, &stable_exec_path(exe.to_path_buf()), iv)? {
        bail!(
            "could not enable the timer; run: systemctl --user enable --now appofholding-update.timer"
        );
    }
    Ok(())
}

// ---- plans

fn size_of(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    fs::read_dir(path)
        .map(|rd| rd.flatten().map(|e| size_of(&e.path())).sum())
        .unwrap_or(0)
}

fn plan(summary: String, changes: Vec<Value>, refuse: Option<&str>) -> Value {
    json!({
        "version": 1,
        "ok": true,
        "summary": summary,
        "changes": changes,
        "would_refuse": refuse.is_some(),
        "reason": refuse.unwrap_or(""),
    })
}

fn bytes_text(b: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1000.0 && i < UNITS.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 {
        format!("{b} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

/// What `remove` would do to `id` with these options, without doing any of it.
pub fn remove_plan(lib: &Library, id: &str, files: FilePolicy, purge_data: bool) -> Result<Value> {
    let rec: AppRecord = lib.get(id)?;
    let delete_file = match files {
        FilePolicy::Delete => true,
        FilePolicy::Keep => false,
        FilePolicy::Auto => !rec.external_file,
    };
    let mut changes = vec![json!({
        "group": "Menu",
        "label": format!("{} in the app menu", rec.name),
        "detail": rec.desktop_file.display().to_string(),
        "reversible": true,
    })];
    if let Some(icon) = &rec.icon_file {
        changes.push(json!({
            "group": "Menu",
            "label": "Its icon",
            "detail": icon.display().to_string(),
            "bytes": size_of(icon),
            "reversible": true,
        }));
    }
    if delete_file {
        changes.push(json!({
            "group": "AppImage",
            "label": rec.appimage_path.file_name().map_or_else(
                || "The AppImage".to_string(), |n| n.to_string_lossy().into_owned()),
            "detail": rec.appimage_path.display().to_string(),
            "bytes": rec.size,
            "reversible": false,
        }));
    }
    let mut kept = 0usize;
    for v in lib.versions(id)? {
        kept += 1;
        changes.push(json!({
            "group": "Kept versions",
            "label": format!("Version {}", v.version.as_deref().unwrap_or("?")),
            "detail": v.path.display().to_string(),
            "bytes": size_of(&v.path),
            "reversible": false,
        }));
    }
    let home = lib.dirs.private_home(id);
    if purge_data && home.is_dir() {
        changes.push(json!({
            "group": "Its data",
            "label": "Settings and files in its private home",
            "detail": home.display().to_string(),
            "bytes": size_of(&home),
            "reversible": false,
        }));
    }
    let mut summary = format!("Remove {}", rec.name);
    if !delete_file {
        summary += &format!("; the AppImage stays at {}", rec.appimage_path.display());
    }
    if !purge_data && home.is_dir() {
        summary += &format!("; its data in {} stays", home.display());
    }
    if kept > 0 {
        summary += &format!("; {kept} kept version(s) go too");
    }
    Ok(plan(summary, changes, None))
}

/// The apps `update` would install (all with an update that counts, or `id` when named), with
/// the version each goes from and to. Reads only what the last check saved.
pub fn update_plan(lib: &Library, id: Option<&str>, force: bool) -> Result<Value> {
    let sources = lib.sources()?;
    let pick = |s: &SourceRecord| id.is_none_or(|i| s.app_id == i);
    let mut changes = Vec::new();
    let mut total = 0u64;
    for s in sources.iter().filter(|s| pick(s)) {
        let latest = s.last_latest.as_deref();
        if !force && !engine::counts_as_update(s.last_status.as_deref(), latest, Some(s)) {
            continue;
        }
        let Ok(app) = lib.get(&s.app_id) else {
            continue;
        };
        let from = app.version.clone().or(s.installed_version.clone());
        total += app.size;
        changes.push(json!({
            "group": "Updates",
            "label": app.name,
            "detail": format!(
                "{} \u{2192} {}",
                from.as_deref().unwrap_or("?"),
                latest.unwrap_or("latest")
            ),
            "reversible": true,
        }));
    }
    let refuse = (changes.is_empty()).then_some("There is nothing to update.");
    let n = changes.len();
    let summary = if n == 0 {
        "Nothing to update".to_string()
    } else {
        format!(
            "{n} update{}; the version each replaces is kept for rollback (about {} per app)",
            if n == 1 { "" } else { "s" },
            bytes_text(total / n as u64)
        )
    };
    Ok(plan(summary, changes, refuse))
}

// ---- events

/// One progress event on its own line. Stdout is line-buffered, so each reaches the reader as
/// it happens.
pub fn event(v: &Value) {
    println!("{v}");
}

/// A progress callback for downloads that emits at most one event per percent.
pub fn download_events(app: String) -> impl FnMut(u64, Option<u64>) {
    let mut last = -1i64;
    move |done, total| {
        let Some(t) = total.filter(|t| *t > 0) else {
            return;
        };
        let pct = (done * 100 / t) as i64;
        if pct != last {
            last = pct;
            event(&json!({ "event": "progress", "app": app, "percent": pct }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_plainly() {
        assert_eq!(bytes_text(12), "12 B");
        assert_eq!(bytes_text(1_500_000), "1.5 MB");
    }
}
