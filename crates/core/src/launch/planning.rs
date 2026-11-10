//! Turning a profile into a launch plan.

use super::*;

/// Build the command to run `appimage` with `profile`. `passthrough` is what the desktop or
/// the user supplied after the app id (files, URLs, the AppImage's own default flags).
///
/// Order: `[elevator] [prefix...] appimage [preset args] [profile args] [passthrough...]`.
/// Explicit environment overrides preset environment.
pub fn plan(
    appimage: &Path,
    profile: &Profile,
    passthrough: &[String],
    elevator: Option<Elevator>,
    session_env: &[(String, String)],
) -> Result<Plan> {
    let Resolved { env, app_args, .. } = resolve(profile, passthrough)?;
    plan_from(appimage, profile, env, app_args, elevator, session_env)
}

/// A profile with presets expanded: everything except how the program gets started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub env: BTreeMap<String, String>,
    /// Preset args, then profile args, then the pass-through args.
    pub app_args: Vec<String>,
    pub prefix: Vec<String>,
    pub cwd: Option<PathBuf>,
}

pub fn resolve(profile: &Profile, passthrough: &[String]) -> Result<Resolved> {
    profile.validate()?;
    let mut env: BTreeMap<String, String> = BTreeMap::new();
    let mut app_args: Vec<String> = Vec::new();
    for name in &profile.presets {
        let p = preset(name).expect("validated");
        app_args.extend(p.args.iter().map(|a| a.to_string()));
        env.extend(p.env.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    }
    app_args.extend(profile.args.iter().cloned());
    app_args.extend(passthrough.iter().cloned());
    env.extend(profile.env.clone());
    Ok(Resolved {
        env,
        app_args,
        prefix: profile.prefix.clone(),
        cwd: profile.cwd.clone(),
    })
}

pub(super) fn plan_from(
    appimage: &Path,
    profile: &Profile,
    mut env: BTreeMap<String, String>,
    app_args: Vec<String>,
    elevator: Option<Elevator>,
    session_env: &[(String, String)],
) -> Result<Plan> {
    if profile.extract_and_run {
        // Understood by every type-2 runtime; avoids needing FUSE.
        env.insert("APPIMAGE_EXTRACT_AND_RUN".into(), "1".into());
    }

    let mut argv: Vec<String> = Vec::new();
    let mut plan_env: Vec<(String, String)> = Vec::new();
    let mut cwd = profile.cwd.clone();

    if profile.elevated {
        let tool = elevator.ok_or_else(|| {
            Error::Other(
                "this profile runs with elevated privileges, but neither run0 nor pkexec was found"
                    .into(),
            )
        })?;
        let mut all: Vec<(String, String)> = session_env.to_vec();
        all.extend(env);
        match tool {
            Elevator::Run0 => {
                argv.push("run0".into());
                for (k, v) in &all {
                    argv.push(format!("--setenv={k}={v}"));
                }
                if let Some(d) = cwd.take() {
                    argv.push(format!("--chdir={}", d.to_string_lossy()));
                }
            }
            Elevator::Pkexec => {
                // pkexec scrubs the environment, so pass it through `env`. It cannot change directory.
                argv.extend(["pkexec".into(), "env".into()]);
                argv.extend(all.iter().map(|(k, v)| format!("{k}={v}")));
                cwd = None;
            }
        }
    } else {
        plan_env = env.into_iter().collect();
    }

    argv.extend(profile.prefix.iter().cloned());
    argv.push(appimage.to_string_lossy().into_owned());
    argv.extend(app_args);
    Ok(Plan {
        argv,
        env: plan_env,
        cwd,
        ..Default::default()
    })
}
