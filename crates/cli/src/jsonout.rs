//! The JSON (and NDJSON) answers that uQ's Settings page and scripts read.

use super::*;

/// Runs a command whose `--json` answer is one object; a failure prints the error envelope and
/// exits 1, so a caller that parses stdout whatever the exit code gets the reason.
pub(crate) fn json_or_exit(r: Result<serde_json::Value>) {
    match r {
        Ok(v) => println!("{v}"),
        Err(e) => {
            pagejson::print_error(&e);
            std::process::exit(1);
        }
    }
}

/// `update --ndjson`: a line when an app starts, one per percent downloaded, one with its result,
/// and a last one with the totals. Failures are results, not a non-zero exit, so the caller
/// reads every line either way; the exit code is 1 only when something failed.
pub(crate) fn update_ndjson(
    lib: &Library,
    http: &UreqHttp,
    ids: &[String],
    opts: &ApplyOptions,
) -> Result<()> {
    let (mut updated, mut failed) = (0, 0);
    for (n, i) in ids.iter().enumerate() {
        let name = lib.get(i).map(|a| a.name).unwrap_or_else(|_| i.clone());
        pagejson::event(&json!({
            "event": "start", "app": i, "name": name, "index": n + 1, "total": ids.len()
        }));
        let result = lib.update(http, i, opts, &mut pagejson::download_events(i.clone()));
        let (outcome, mut extra) = match result {
            Ok(Applied::Updated { from, to, delta }) => {
                updated += 1;
                ("updated", json!({ "from": from, "to": to, "delta": delta }))
            }
            Ok(Applied::NoUpdate(Status::Unknown)) => ("unknown", json!({})),
            Ok(Applied::NoUpdate(_)) => ("current", json!({})),
            Ok(Applied::Skipped(why)) => ("skipped", json!({ "message": why.to_string() })),
            Ok(Applied::Deferred { version, pids }) => {
                ("deferred", json!({ "to": version, "pids": pids }))
            }
            Err(e) => {
                failed += 1;
                ("failed", json!({ "message": e.to_string() }))
            }
        };
        let obj = extra.as_object_mut().unwrap();
        obj.insert("event".into(), "result".into());
        obj.insert("app".into(), json!(i));
        obj.insert("name".into(), json!(name));
        obj.insert("outcome".into(), outcome.into());
        pagejson::event(&extra);
    }
    pagejson::event(
        &json!({ "event": "done", "updated": updated, "failed": failed, "total": ids.len() }),
    );
    if failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}

/// `add --json`: the same steps as `add`, but nothing on stdout except the answer.
pub(crate) fn add_json(
    target: String,
    asset: Option<String>,
    channel: Option<String>,
    move_file: bool,
    id: Option<String>,
    post_install: Option<String>,
) -> Result<serde_json::Value> {
    let opts = AddOptions {
        install: if move_file {
            Install::Move
        } else {
            Install::Copy
        },
        id,
    };
    let lib = library()?;
    let path = PathBuf::from(&target);
    if path.is_file() && (asset.is_some() || channel.is_some()) {
        bail!("--asset and --channel only apply when adding from a link");
    }
    let rec = if path.is_file() {
        lib.add(&path, &opts)?
    } else {
        let parsed = build_source(&target, asset, channel)?;
        lib.install_from_source(&UreqHttp::new(), parsed, &opts, &mut |_, _| {})?
            .0
    };
    if let Some(cmd) = post_install {
        lib.set_hook(&rec.id, "post-install", &cmd)?;
        lib.run_hook(&rec.id, "post-install")?;
    }
    Ok(json!({
        "version": 1,
        "ok": true,
        "id": rec.id,
        "name": rec.name,
        "app_version": rec.version,
        "file": rec.appimage_path.display().to_string(),
        "updates": lib.source(&rec.id)?.map(|s| s.source.describe()),
    }))
}

// ---------- JSON output (`list --json`, `check --json`, `schedule status --json`) ----------
//
// For scripts and desktop integrations (uq/ in this repository). Field names are snake_case and
// stable; `version` changes only if a field changes meaning.

pub(crate) fn list_json(apps: &[AppRecord], sources: &[SourceRecord]) -> serde_json::Value {
    let by_id: std::collections::HashMap<&str, &SourceRecord> =
        sources.iter().map(|s| (s.app_id.as_str(), s)).collect();
    let mut updates = 0;
    let rows: Vec<serde_json::Value> = apps
        .iter()
        .map(|a| {
            let src = by_id.get(a.id.as_str()).copied();
            let status = src.and_then(|s| s.last_status.as_deref());
            let latest = src.and_then(|s| s.last_latest.as_deref());
            let update = engine::counts_as_update(status, latest, src);
            updates += usize::from(update);
            json!({
                "id": a.id,
                "name": a.name,
                "version": a.version.clone().or(src.and_then(|s| s.installed_version.clone())),
                "desktop_id": format!("{}.desktop", appofholding_core::paths::desktop_id(&a.id)),
                "appimage": a.appimage_path.display().to_string(),
                "size_bytes": a.size,
                // The icon theme name of the menu entry's icon, when the app has one.
                "icon": a.icon_file.as_ref().map(|_| appofholding_core::paths::desktop_id(&a.id)),
                "external": a.external_file,
                "has_source": src.is_some(),
                "source": src.map(|s| s.source.describe()),
                "policy": src.map(|s| s.policy.as_str()),
                "status": status,
                "latest": latest,
                "skipped": src.and_then(|s| s.skipped_version.clone()),
                "error": src.and_then(|s| s.last_error.clone()),
                "last_checked": src.and_then(|s| s.last_checked),
                "update": update,
            })
        })
        .collect();
    json!({ "version": 1, "apps": rows, "updates": updates })
}

pub(crate) fn check_row(
    id: &str,
    result: &std::result::Result<Report, appofholding_core::Error>,
    src: Option<&SourceRecord>,
) -> serde_json::Value {
    match result {
        Ok(r) => json!({
            "id": id,
            "status": r.status.as_str(),
            "latest": r.latest_version(),
            "problem": r.problem(),
            "error": null,
            "update": report_is_update(r, src),
        }),
        Err(e) => json!({
            "id": id,
            "status": null,
            "latest": null,
            "problem": null,
            "error": e.to_string(),
            "update": false,
        }),
    }
}

/// A fresh check's answer counted the way `list --json` counts stored ones: an update the user
/// can act on, so not ambiguous, not pinned and not the skipped version.
pub(crate) fn report_is_update(r: &Report, src: Option<&SourceRecord>) -> bool {
    r.problem().is_none()
        && engine::counts_as_update(Some(r.status.as_str()), Some(r.latest_version()), src)
}

pub(crate) fn check_json(
    results: &[(
        String,
        std::result::Result<Report, appofholding_core::Error>,
    )],
    sources: &[SourceRecord],
) -> serde_json::Value {
    let by_id: std::collections::HashMap<&str, &SourceRecord> =
        sources.iter().map(|s| (s.app_id.as_str(), s)).collect();
    let rows: Vec<serde_json::Value> = results
        .iter()
        .map(|(i, r)| check_row(i, r, by_id.get(i.as_str()).copied()))
        .collect();
    let updates = rows.iter().filter(|r| r["update"] == true).count();
    json!({ "version": 1, "apps": rows, "updates": updates })
}

/// One line for a notification about one app's check.
pub(crate) fn check_summary(
    result: &std::result::Result<Report, appofholding_core::Error>,
    src: Option<&SourceRecord>,
) -> String {
    match result {
        Ok(r) => match (r.problem(), r.status) {
            (Some(p), _) => format!("Could not choose the update: {p}"),
            (None, Status::UpdateAvailable) if !report_is_update(r, src) => {
                let why = if src.is_some_and(|s| s.policy == Policy::Pinned) {
                    "the app is pinned to the installed version"
                } else {
                    "you chose to skip it"
                };
                format!("Version {} is available, but {why}.", r.latest_version())
            }
            (None, Status::UpdateAvailable) => {
                format!("Version {} is available.", r.latest_version())
            }
            (None, Status::UpToDate) => format!("Up to date (latest: {}).", r.latest_version()),
            (None, Status::Unknown) => format!(
                "Can't tell whether {} is newer than the installed version.",
                r.latest_version()
            ),
        },
        Err(e) => format!("Could not check for updates: {e}"),
    }
}

/// `interval` is `None` when the timer is missing or was edited to another calendar.
pub(crate) fn schedule_json(installed: bool, interval: Option<Interval>) -> serde_json::Value {
    json!({
        "version": 1,
        "installed": installed,
        "interval": interval.filter(|_| installed).map(Interval::as_str),
    })
}
