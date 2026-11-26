//! `self`, `schedule`, `defaults` and `config`: the app's own updates and settings.

use super::*;

pub(crate) fn self_cmd(cmd: SelfCmd) -> Result<()> {
    let dirs = dirs()?;
    let http = UreqHttp::new();
    let install = selfupdate::detect_current();
    let arch = Arch::host();
    let how = match &install {
        selfupdate::InstallKind::Pacman { package } => {
            format!("installed with pacman (package {package})")
        }
        selfupdate::InstallKind::AppImage { path } => {
            format!("running from the AppImage {}", path.display())
        }
        selfupdate::InstallKind::UserLocal { path } => {
            format!("installed under your home folder ({})", path.display())
        }
        selfupdate::InstallKind::Unknown => "installation method unknown".to_string(),
    };
    match cmd {
        SelfCmd::Check { force } => {
            let s = selfupdate::check_cached(&http, &dirs, install, arch, force)?;
            println!("appOFholding {}  ({how})", s.current);
            if s.update_available {
                println!(
                    "A newer release is available: {}{}",
                    s.latest,
                    if s.from_cache { "  (cached)" } else { "" }
                );
                println!("  {}", s.release_url);
                println!(
                    "Update with: appofholding self update{}",
                    if matches!(s.install, selfupdate::InstallKind::Pacman { .. }) {
                        " --install"
                    } else {
                        ""
                    }
                );
            } else {
                println!("Up to date (latest release: {}).", s.latest);
            }
        }
        SelfCmd::Update {
            install: run_install,
        } => {
            let status = selfupdate::check(&http, install, arch)?;
            let work = dirs.cache_home.join("appofholding/self");
            match selfupdate::apply(&http, &status, &work, &mut progress())? {
                selfupdate::Applied::AlreadyCurrent => {
                    println!("appOFholding {} is the newest release.", status.current)
                }
                selfupdate::Applied::Replaced { to, path, backup } => {
                    println!("Updated to {to}: {}", path.display());
                    println!(
                        "The previous version is kept at {} (delete it when you are happy).",
                        backup.display()
                    );
                }
                selfupdate::Applied::PackageReady { to, file, command } => {
                    println!(
                        "Downloaded and verified appOFholding {to}: {}",
                        file.display()
                    );
                    let line = command.join(" ");
                    if run_install {
                        println!("Running: {line}");
                        let ok = Command::new(&command[0])
                            .args(&command[1..])
                            .status()
                            .is_ok_and(|s| s.success());
                        if !ok {
                            bail!("the package manager did not finish; you can retry with: {line}");
                        }
                        let _ = std::fs::remove_file(&file);
                        println!("Installed {to}. Restart any open appOFholding windows.");
                    } else {
                        println!("Install it with:\n  {line}\n(or re-run with --install)");
                    }
                }
                selfupdate::Applied::Manual { to, instructions } => {
                    println!("Version {to} is available.\n{instructions}")
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn defaults_cmd(cmd: DefaultsCmd) -> Result<()> {
    let lib = library()?;
    match cmd {
        DefaultsCmd::Show { json } => {
            let d = lib.defaults()?;
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "policy": d.policy.as_str(),
                        "keepVersions": d.keep_versions,
                        "sandbox": d.sandbox.as_str(),
                    })
                );
            } else {
                println!("policy         {}", d.policy.as_str());
                println!("keep-versions  {}", d.keep_versions);
                println!("sandbox        {}", d.sandbox.as_str());
            }
        }
        DefaultsCmd::Set {
            policy,
            all,
            keep_versions,
            sandbox,
        } => {
            if policy.is_none() && keep_versions.is_none() && sandbox.is_none() {
                bail!("nothing to set: give --policy, --keep-versions or --sandbox");
            }
            // Check every value before changing anything.
            let policy = policy
                .map(|p| {
                    Policy::parse(&p)
                        .ok_or_else(|| anyhow!("policy must be auto, notify, manual or pinned"))
                })
                .transpose()?;
            let sandbox = sandbox
                .map(|l| match Level::parse(&l) {
                    Some(l @ (Level::None | Level::Standard | Level::Strict)) => Ok(l),
                    _ => Err(anyhow!("sandbox must be none, standard or strict")),
                })
                .transpose()?;
            if let Some(n) = keep_versions
                && n > appofholding_core::defaults::MAX_KEEP_VERSIONS
            {
                bail!(
                    "at most {} previous versions can be kept",
                    appofholding_core::defaults::MAX_KEEP_VERSIONS
                );
            }
            if let Some(p) = policy {
                lib.set_default_policy(p)?;
                println!("New update sources start as: {}", p.as_str());
                if all {
                    let n = lib.set_policy_all(p)?;
                    println!("Changed {n} app(s) to {}", p.as_str());
                }
            }
            if let Some(n) = keep_versions {
                lib.set_keep_versions(n)?;
                println!("Updates keep {n} previous version(s)");
            }
            if let Some(l) = sandbox {
                lib.set_default_sandbox(l)?;
                println!("New apps start with sandbox: {}", l.as_str());
            }
        }
    }
    Ok(())
}

pub(crate) fn config_cmd(cmd: ConfigCmd) -> Result<()> {
    let json = cmd.json();
    let exe = std::env::current_exe().context("cannot determine own path")?;
    let lib = match library() {
        Ok(l) => l,
        Err(e) if json => {
            pagejson::print_error(&e);
            std::process::exit(1);
        }
        Err(e) => return Err(e),
    };
    match cmd {
        ConfigCmd::Show { json: true } => json_or_exit(pagejson::show_json(&lib)),
        ConfigCmd::Show { json: false } => pagejson::show_text(&lib)?,
        ConfigCmd::Set {
            key,
            value,
            all,
            json,
        } => {
            let r = pagejson::set(&lib, &exe, &key, &value, all);
            if json {
                json_or_exit(r);
            } else {
                r?;
                println!("{key} is now {value}");
            }
        }
        ConfigCmd::Reset { key, json } => {
            let r = pagejson::reset(&lib, &exe, &key);
            if json {
                json_or_exit(r);
            } else {
                r?;
                println!("{key} is back to its built-in value");
            }
        }
    }
    Ok(())
}

pub(crate) fn schedule_cmd(cmd: ScheduleCmd) -> Result<()> {
    let dirs = dirs()?;
    match cmd {
        ScheduleCmd::Install { interval } => {
            let Some(iv) = Interval::parse(&interval) else {
                bail!("interval must be hourly, daily or weekly")
            };
            let exe = std::env::current_exe().context("cannot determine own path")?;
            let exe = appofholding_core::paths::stable_exec_path(exe);
            let enabled = schedule::enable(&dirs, &exe, iv)?;
            let (svc, timer) = schedule::unit_paths(&dirs);
            println!("Wrote {}\n      {}", svc.display(), timer.display());
            if dirs.refresh_caches {
                if enabled {
                    println!("Background checks are on ({interval}).");
                } else {
                    println!(
                        "Could not enable the timer; run: systemctl --user enable --now appofholding-update.timer"
                    );
                }
            }
        }
        ScheduleCmd::Remove => {
            let removed = schedule::disable(&dirs)?;
            println!(
                "{}",
                if removed {
                    "Background checks are off."
                } else {
                    "Background checks were not installed."
                }
            );
        }
        ScheduleCmd::Status { json: true } => {
            println!(
                "{}",
                schedule_json(
                    schedule::installed(&dirs),
                    schedule::installed_interval(&dirs)
                )
            );
        }
        ScheduleCmd::Status { json: false } => {
            match (
                schedule::installed(&dirs),
                schedule::installed_interval(&dirs),
            ) {
                (true, Some(iv)) => println!("units:  installed ({})", iv.as_str()),
                (true, None) => println!("units:  installed"),
                (false, _) => println!("units:  not installed"),
            }
            if dirs.refresh_caches {
                schedule::show_status();
            }
        }
    }
    Ok(())
}
