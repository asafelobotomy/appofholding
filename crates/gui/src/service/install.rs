//! Getting apps in: links, dropped files and the import dialog.

use super::*;

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

pub(crate) fn mode_from(text: &str) -> Res<Option<Install>> {
    Ok(match text {
        "" | "default" => None,
        "in-place" => Some(Install::InPlace),
        "copy" => Some(Install::Copy),
        "move" => Some(Install::Move),
        other => return Err(format!("unknown import mode '{other}'")),
    })
}

pub(crate) fn mode_name(m: Install) -> &'static str {
    match m {
        Install::InPlace => "in-place",
        Install::Copy => "copy",
        Install::Move => "move",
    }
}

pub(crate) fn import_plan_for(
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
                Action::Update { app_id, from, to } => (
                    "update",
                    format!(
                        "updates {app_id} from {} to {}",
                        from.as_deref().unwrap_or("?"),
                        to.as_deref().unwrap_or("?")
                    ),
                    app_id.clone(),
                ),
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
/// Update app `id` from the AppImage at `path` (a newer version the user dropped or opened):
/// `mode` is "move" (the default) or "copy". {id, name, from, to}.
pub fn update_from_file(lib: &Library, id: &str, path: &str, mode: &str) -> Res<Value> {
    let path = local_path(path);
    let install = match mode_from(mode)? {
        Some(Install::Copy) => Install::Copy,
        _ => Install::Move,
    };
    let from = lib.get(id).map_err(e)?.version;
    let rec = lib.update_from_file(id, &path, install).map_err(e)?;
    Ok(json!({"id": rec.id, "name": rec.name, "from": from, "to": rec.version}))
}

pub fn add_file(lib: &Library, path: &str, mode: &str) -> Res<Value> {
    let path = local_path(path);
    let install = mode_from(mode)?.unwrap_or(Install::Copy);
    let rec = lib
        .add(&path, &AddOptions { install, id: None })
        .map_err(e)?;
    Ok(json!({"id": rec.id, "name": rec.name, "moved": install == Install::Move}))
}

/// What the window needs to know about files dropped on it before asking where they should
/// go: for each, its absolute path, its file name, whether it already sits in the library
/// folder (then there is nothing to ask), and `known` when it is another version of an app
/// already in the library ({id, name, installed, version, relation: newer | same | older |
/// unknown}), which is offered as an update rather than added. Folders and missing files are
/// left out.
pub fn describe_drop(lib: &Library, paths: &[String]) -> Value {
    let lib_dir = &lib.dirs.library_dir;
    let lib_dir = std::fs::canonicalize(lib_dir).unwrap_or_else(|_| lib_dir.to_path_buf());
    Value::Array(
        paths
            .iter()
            .map(|p| local_path(p))
            .filter(|p| p.is_file())
            .map(|p| {
                let real = std::fs::canonicalize(&p).unwrap_or_else(|_| p.clone());
                let known = lib.local_version_of(&p).ok().flatten().map(|k| {
                    json!({
                        "id": k.id,
                        "name": k.name,
                        "installed": k.installed,
                        "version": k.version,
                        "relation": k.relation.as_str(),
                    })
                });
                json!({
                    "path": p.display().to_string(),
                    "name": p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                    "inLibrary": real.parent() == Some(lib_dir.as_path()),
                    "known": known,
                })
            })
            .collect(),
    )
}
