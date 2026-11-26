//! Checking for and installing updates, of the apps and of App of Holding itself.

use super::*;

pub(crate) fn self_json(s: &appofholding_core::selfupdate::SelfStatus) -> Value {
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

pub fn check_app(lib: &Library, http: &dyn appofholding_core::http::Http, id: &str) -> Res<Value> {
    let r = lib.check(http, id).map_err(e)?;
    Ok(json!({"status": r.status.as_str(), "latest": r.latest_version(), "problem": r.problem()}))
}

#[cfg(test)]
pub fn check_all(lib: &Library, http: &(dyn appofholding_core::http::Http + Sync)) -> Res<Value> {
    check_all_with(lib, http, false)
}

/// Check every app with a source. `cached` sends the stored ETags (background checks), so an
/// unchanged release is not fetched again; a forced check always asks.
pub(crate) fn check_all_with(
    lib: &Library,
    http: &(dyn appofholding_core::http::Http + Sync),
    cached: bool,
) -> Res<Value> {
    let ids: Vec<String> = lib
        .sources()
        .map_err(e)?
        .into_iter()
        .map(|s| s.app_id)
        .collect();
    // (status, latest, problem) per app.
    type Outcome = (String, Option<String>, Option<String>);
    let checked: Vec<(String, appofholding_core::Result<Outcome>)> = if cached {
        lib.check_many_cached(http, &ids)
            .into_iter()
            .map(|(id, r)| {
                let r = r.map(|st| match st.report {
                    Some(r) => (
                        r.status.as_str().to_string(),
                        Some(r.latest_version().to_string()),
                        r.problem(),
                    ),
                    None => (st.status.as_str().to_string(), st.latest, None),
                });
                (id, r)
            })
            .collect()
    } else {
        lib.check_many(http, &ids)
            .into_iter()
            .map(|(id, r)| {
                let r = r.map(|r| {
                    (
                        r.status.as_str().to_string(),
                        Some(r.latest_version().to_string()),
                        r.problem(),
                    )
                });
                (id, r)
            })
            .collect()
    };
    // Read after the check, so a pin or skip is as current as the answer.
    let sources = lib.sources().map_err(e)?;
    let results = checked
        .into_iter()
        .map(|(id, r)| match r {
            Ok((status, latest, problem)) => {
                let src = sources.iter().find(|s| s.app_id == id);
                let update = problem.is_none()
                    && engine::counts_as_update(Some(status.as_str()), latest.as_deref(), src);
                json!({"id": id, "status": status, "latest": latest, "problem": problem, "update": update})
            }
            Err(err) => json!({"id": id, "error": err.to_string()}),
        })
        .collect();
    Ok(Value::Array(results))
}

/// The tray's background check: check every app, then hand out the updates the user has not
/// been told about yet ({results, fresh: [{id, name, version}]}).
/// `forced`: asked for by the user, so every source is asked again rather than trusting an
/// unchanged ETag.
pub fn notify_check(
    lib: &Library,
    http: &(dyn appofholding_core::http::Http + Sync),
    forced: bool,
) -> Res<Value> {
    let results = check_all_with(lib, http, !forced)?;
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
        keep_versions: lib.keep_versions().map_err(e)?,
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
