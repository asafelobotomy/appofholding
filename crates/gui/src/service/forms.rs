//! The window's forms: launch profiles, sandbox settings and library defaults, read and saved.

use super::*;

pub(crate) fn sandbox_json(s: &Settings) -> Value {
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

pub(crate) fn str_of<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

pub(crate) fn bool_of(v: &Value, k: &str) -> bool {
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

/// The settings that apply to every app: {policy, keepVersions, maxKeepVersions, sandbox, scan}.
pub fn library_defaults(lib: &Library) -> Res<Value> {
    let d = lib.defaults().map_err(e)?;
    Ok(json!({
        "policy": d.policy.as_str(),
        "keepVersions": d.keep_versions,
        "maxKeepVersions": appofholding_core::defaults::MAX_KEEP_VERSIONS,
        "sandbox": d.sandbox.as_str(),
        "scan": lib.scan_mode().map_err(e)?.as_str(),
    }))
}

/// Change any of `policy`, `keepVersions`, `sandbox` and `scan`; keys left out stay as they are.
/// Every value is checked before anything is saved.
pub fn set_library_defaults(lib: &Library, v: &Value) -> Res<()> {
    let policy = match v.get("policy").and_then(Value::as_str) {
        Some(p) => Some(Policy::parse(p).ok_or("policy must be auto, notify, manual or pinned")?),
        None => None,
    };
    let sandbox = match v.get("sandbox").and_then(Value::as_str) {
        Some(l) => match Level::parse(l) {
            Some(l @ (Level::None | Level::Standard | Level::Strict)) => Some(l),
            _ => return Err("sandbox must be none, standard or strict".into()),
        },
        None => None,
    };
    let scan = match v.get("scan").and_then(Value::as_str) {
        Some(m) => Some(
            appofholding_core::scan::ScanMode::parse(m).ok_or("scan must be off, warn or block")?,
        ),
        None => None,
    };
    let keep = match v.get("keepVersions") {
        Some(n) => Some(
            n.as_u64()
                .filter(|n| *n as usize <= appofholding_core::defaults::MAX_KEEP_VERSIONS)
                .ok_or("keepVersions must be a whole number from 0 to 10")? as usize,
        ),
        None => None,
    };
    if let Some(p) = policy {
        lib.set_default_policy(p).map_err(e)?;
    }
    if let Some(l) = sandbox {
        lib.set_default_sandbox(l).map_err(e)?;
    }
    if let Some(m) = scan {
        lib.set_scan_mode(m).map_err(e)?;
    }
    if let Some(n) = keep {
        lib.set_keep_versions(n).map_err(e)?;
    }
    Ok(())
}
