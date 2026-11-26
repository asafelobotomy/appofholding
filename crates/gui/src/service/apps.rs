//! The library as the window sees it: apps, their details and the actions on them.

use super::*;

pub(crate) fn icon_url(a: &AppRecord) -> String {
    a.icon_file
        .as_ref()
        .map(|p| format!("file://{}", p.display()))
        .unwrap_or_default()
}

/// Where updates come from, short enough for a list row.
pub(crate) fn short_source(p: &source::Provider) -> String {
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
            // What oysterAV last said about this build, or null.
            "scan": lib.scan_verdict(&a.id).map_err(e)?.map(|v| json!({
                "state": v.state.as_str(),
                "detail": v.detail,
            })),
            "update": engine::counts_as_update(
                src.and_then(|s| s.last_status.as_deref()),
                src.and_then(|s| s.last_latest.as_deref()),
                src,
            ),
        }));
    }
    Ok(Value::Array(out))
}

pub(crate) fn profile_json(p: &Profile) -> Value {
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
        "scan": lib.scan_verdict(&a.id).map_err(e)?.map(|v| json!({
            "state": v.state.as_str(),
            "detail": v.detail,
        })),
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
            "update": engine::counts_as_update(s.last_status.as_deref(), s.last_latest.as_deref(), Some(s)),
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

pub fn set_policy(lib: &Library, id: &str, policy: &str) -> Res<()> {
    let p = Policy::parse(policy).ok_or("policy must be auto, notify, manual or pinned")?;
    lib.set_policy(id, p).map_err(e)
}

/// Have oysterAV scan one app's AppImage and keep the answer: {state, detail}. Waits for it, so
/// it runs as a task, not on the interface thread.
pub fn scan_app(lib: &Library, id: &str) -> Res<Value> {
    let v = lib.scan_app(id).map_err(e)?;
    Ok(json!({ "state": v.state.as_str(), "detail": v.detail }))
}

/// Give every app with an update source `policy`: {changed}.
pub fn apply_policy_all(lib: &Library, policy: &str) -> Res<Value> {
    let p = Policy::parse(policy).ok_or("policy must be auto, notify, manual or pinned")?;
    Ok(json!({ "changed": lib.set_policy_all(p).map_err(e)? }))
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

pub fn apply_suggestion(
    lib: &Library,
    http: &dyn appofholding_core::http::Http,
    id: &str,
    key: &str,
) -> Res<Value> {
    let message = lib.apply_suggestion(http, id, key).map_err(e)?;
    Ok(json!({ "message": message }))
}

pub fn dismiss_suggestion(lib: &Library, id: &str, key: &str) -> Res<()> {
    lib.dismiss_suggestion(id, key).map_err(e)
}
