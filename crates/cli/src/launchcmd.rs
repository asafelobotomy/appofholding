//! `launch` and `hook`: launch profiles and update hooks.

use super::*;

pub(crate) fn launch_ctx(unsandboxed: bool) -> LaunchCtx {
    LaunchCtx {
        elevator: launch::detect_elevator(),
        session_env: launch::SESSION_ENV
            .iter()
            .filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v)))
            .collect(),
        host: None,
        unsandboxed,
    }
}

pub(crate) fn kv(s: &str) -> Result<(String, String)> {
    s.split_once('=')
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .with_context(|| format!("'{s}' should look like NAME=value"))
}

pub(crate) fn launch_cmd(cmd: LaunchCmd) -> Result<()> {
    match cmd {
        LaunchCmd::Presets => {
            for p in launch::PRESETS {
                println!("{:<18} {}", p.name, p.description);
                let mut what: Vec<String> = p.args.iter().map(|a| a.to_string()).collect();
                what.extend(p.env.iter().map(|(k, v)| format!("{k}={v}")));
                if !what.is_empty() {
                    println!("{:<18} adds: {}", "", what.join(" "));
                }
            }
        }
        LaunchCmd::Profiles { id } => {
            let lib = library()?;
            let list = lib.profiles(&id)?;
            if list.is_empty() {
                println!("{id}: nothing configured (default profile is empty)");
            }
            for (name, p) in list {
                println!(
                    "{name}: {} arg(s), {} env var(s), presets [{}]",
                    p.args.len(),
                    p.env.len(),
                    p.presets.join(", ")
                );
            }
        }
        LaunchCmd::Show { id, profile } => {
            let lib = library()?;
            let name = profile.as_deref().unwrap_or(launch::DEFAULT_PROFILE);
            let p = lib.profile(&id, name)?;
            println!("profile:        {name}");
            println!(
                "presets:        {}",
                if p.presets.is_empty() {
                    "-".into()
                } else {
                    p.presets.join(", ")
                }
            );
            println!(
                "args:           {}",
                if p.args.is_empty() {
                    "-".into()
                } else {
                    desktop::join_exec(&p.args)
                }
            );
            println!(
                "env:            {}",
                if p.env.is_empty() {
                    "-".into()
                } else {
                    p.env
                        .iter()
                        .map(|(k, v)| format!("{k}={v}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                }
            );
            println!(
                "prefix:         {}",
                if p.prefix.is_empty() {
                    "-".into()
                } else {
                    desktop::join_exec(&p.prefix)
                }
            );
            println!(
                "working dir:    {}",
                p.cwd
                    .as_ref()
                    .map_or("-".into(), |d| d.display().to_string())
            );
            println!("extract & run:  {}", p.extract_and_run);
            println!("elevated:       {}", p.elevated);
            match lib.plan_launch(&id, Some(name), &[], &launch_ctx(false)) {
                Ok(plan) => println!("\ncommand:\n  {}", plan.display()),
                Err(e) => println!("\ncommand: ({e})"),
            }
        }
        LaunchCmd::Set {
            id,
            profile,
            args,
            env,
            prefix,
            cwd,
            clear_cwd,
            preset,
            clear_presets,
            extract_and_run,
            elevated,
            reset,
        } => {
            let lib = library()?;
            let name = profile.unwrap_or_else(|| launch::DEFAULT_PROFILE.to_string());
            let mut p = if reset {
                launch::Profile::default()
            } else {
                // A profile that does not exist yet is created; any other failure (no such
                // app, an unreadable database) must not be saved over with an empty profile.
                lib.profiles(&id)?
                    .into_iter()
                    .find(|(n, _)| *n == name)
                    .map(|(_, p)| p)
                    .unwrap_or_default()
            };
            if let Some(a) = args {
                p.args = desktop::split_exec(&a);
            }
            if !env.is_empty() {
                p.env = env.iter().map(|s| kv(s)).collect::<Result<_>>()?;
            }
            if let Some(x) = prefix {
                p.prefix = desktop::split_exec(&x);
            }
            if clear_cwd {
                p.cwd = None;
            }
            if let Some(c) = cwd {
                p.cwd = Some(PathBuf::from(c));
            }
            if clear_presets {
                p.presets.clear();
            }
            if !preset.is_empty() {
                p.presets = preset;
            }
            if let Some(v) = extract_and_run {
                p.extract_and_run = v;
            }
            if let Some(v) = elevated {
                p.elevated = v;
            }
            lib.set_profile(&id, &name, &p)?;
            println!("{id}: saved launch profile '{name}'");
            if p.elevated {
                println!("note: this profile starts the app as root through run0/pkexec");
            }
        }
        LaunchCmd::Delete { id, profile } => {
            let removed = library()?.delete_profile(&id, &profile)?;
            println!(
                "{}",
                if removed {
                    "Profile removed."
                } else {
                    "No such profile."
                }
            );
        }
        LaunchCmd::Hook(h) => hook_cmd(h)?,
    }
    Ok(())
}

pub(crate) fn hook_cmd(cmd: HookCmd) -> Result<()> {
    let lib = library()?;
    match cmd {
        HookCmd::Show { id } => {
            let hooks = lib.hooks(&id)?;
            if hooks.is_empty() {
                println!(
                    "{id}: no hooks set (kinds: {})",
                    profiles::HOOK_KINDS.join(", ")
                );
            }
            for (k, c) in hooks {
                println!("{k:<13} {c}");
            }
        }
        HookCmd::Set { id, kind, command } => {
            lib.set_hook(&id, &kind, &command)?;
            println!("{id}: {kind} hook saved");
        }
        HookCmd::Clear { id, kind } => {
            println!(
                "{}",
                if lib.clear_hook(&id, &kind)? {
                    "Hook removed."
                } else {
                    "No such hook."
                }
            );
        }
        HookCmd::Run { id, kind } => {
            if lib.run_hook(&id, &kind)? {
                println!("{kind} hook finished");
            } else {
                println!("{id} has no {kind} hook");
            }
        }
    }
    Ok(())
}

pub(crate) fn describe_outcome(o: &Outcome) -> String {
    match o {
        Outcome::Updated { id, from, to } => {
            format!("{id}: updated {} -> {to}", from.as_deref().unwrap_or("?"))
        }
        Outcome::Available {
            id,
            version,
            notified,
        } => format!(
            "{id}: {version} available{}",
            if *notified { " (notified)" } else { "" }
        ),
        Outcome::Deferred { id, version } => format!("{id}: {version} waiting, app is running"),
        Outcome::UpToDate { id } => format!("{id}: up to date"),
        Outcome::Skipped { id, reason } => format!("{id}: skipped ({reason})"),
        Outcome::Failed { id, error } => format!("{id}: error: {error}"),
    }
}
