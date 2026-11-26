use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use appofholding_core::assets::{self, Arch, Selection};
use appofholding_core::check::{self, Report};
use appofholding_core::config::{ApplyOptions as ConfigApply, Config, Note};
use appofholding_core::desktop;
use appofholding_core::engine::{self, Applied, ApplyOptions, IfRunning, Outcome};
use appofholding_core::http::UreqHttp;
use appofholding_core::import;
use appofholding_core::launch;
use appofholding_core::notify::Notifier;
use appofholding_core::notify::NotifySend;
use appofholding_core::profiles::{self, LaunchCtx};
use appofholding_core::sandbox::{self, Bus, HomeAccess, Level, PathGrant};
use appofholding_core::schedule::{self, Interval};
use appofholding_core::selfupdate;
use appofholding_core::source::Policy;
use appofholding_core::source::{self, Channel, Source};
use appofholding_core::version::{Installed, Status};
use appofholding_core::{
    AddOptions, AppRecord, Dirs, FilePolicy, Install, Library, SourceRecord, inspect,
};
use clap::{CommandFactory, Parser, Subcommand};
use serde_json::json;

mod apps;
mod args;
mod importcmd;
mod jsonout;
mod launchcmd;
mod pagejson;
mod sandboxcmd;
mod scancmd;
mod settings;
mod sourcecmd;
mod updates;

use apps::*;
use args::*;
use importcmd::*;
use jsonout::*;
use launchcmd::*;
use pagejson::ConfigCmd;
use sandboxcmd::*;
use settings::*;
use sourcecmd::*;
use updates::*;

/// The library app behind a menu entry's desktop id (`appofholding-<id>.desktop`).
fn app_for_desktop_id(lib: &Library, desktop_id: &str) -> Result<String> {
    let Some(id) = appofholding_core::paths::app_id_from_desktop_id(desktop_id) else {
        bail!("{desktop_id} is not a menu entry made by appOFholding");
    };
    lib.get(id)?;
    Ok(id.to_string())
}

fn dirs() -> Result<Dirs> {
    Ok(Dirs::detect()?)
}

fn library() -> Result<Library> {
    let exe = std::env::current_exe().context("cannot determine own path")?;
    Ok(Library::open(
        dirs()?,
        appofholding_core::paths::stable_exec_path(exe),
    )?)
}

/// Under systemd (uQ's launcher starts `appofholding run`), a failure is logged with its code
/// and level for uQ's Settings › Problems; in a terminal it prints as before. A panic leaves a
/// crash record either way.
fn main() -> Result<()> {
    use appofholding_core::uq_diag::{Style, install_panic_hook};
    install_panic_hook("appofholding");
    let r = run();
    if let Err(e) = &r
        && Style::detect() == Style::Journal
    {
        eprintln!("<3>appofholding: [APPOFHOLDING-002] {e:#}");
        std::process::exit(1);
    }
    r
}

fn run() -> Result<()> {
    match Cli::parse().command {
        cmd @ Cmd::Add { .. } => add_cmd(cmd)?,
        cmd @ Cmd::Suggest { .. } => suggest_cmd(cmd)?,
        Cmd::Scan(args) => scancmd::run(&library()?, args)?,
        Cmd::List { json: true } => {
            let lib = library()?;
            let mut doc = list_json(&lib.list()?, &lib.sources()?);
            scancmd::add_verdicts(&lib, &mut doc);
            println!("{doc}");
        }
        Cmd::List { json: false } => {
            let apps = library()?.list()?;
            if apps.is_empty() {
                println!("No apps yet. Add one with: appofholding add <file>");
            }
            for a in apps {
                println!(
                    "{:<24} {:<28} {:<10} {}",
                    a.id,
                    a.name,
                    a.version.as_deref().unwrap_or("-"),
                    a.appimage_path.display()
                );
            }
        }
        cmd @ Cmd::Info { .. } => info_cmd(cmd)?,
        cmd @ Cmd::Remove { .. } => remove_cmd(cmd)?,
        Cmd::Sandbox(cmd) => sandbox_cmd(cmd)?,
        Cmd::Import(args) => import_cmd(args)?,
        Cmd::SelfUpdate(cmd) => self_cmd(cmd)?,
        Cmd::Export { output } => {
            let text = library()?.export_config()?.to_toml()?;
            match output {
                Some(p) => {
                    std::fs::write(&p, text)
                        .with_context(|| format!("cannot write {}", p.display()))?;
                    eprintln!("Wrote {}", p.display());
                }
                None => print!("{text}"),
            }
        }
        cmd @ Cmd::Apply { .. } => apply_cmd(cmd)?,
        Cmd::Completions { shell } => {
            clap_complete::generate(
                shell,
                &mut Cli::command(),
                "appofholding",
                &mut std::io::stdout(),
            );
        }
        Cmd::Man { dir } => {
            std::fs::create_dir_all(&dir)?;
            let cmd = Cli::command();
            let render = |c: clap::Command, name: String| -> Result<()> {
                let mut buf = Vec::new();
                clap_mangen::Man::new(c).render(&mut buf)?;
                std::fs::write(dir.join(format!("{name}.1")), buf)?;
                Ok(())
            };
            render(cmd.clone(), "appofholding".into())?;
            for sub in cmd.get_subcommands().filter(|s| !s.is_hide_set()) {
                render(sub.clone(), format!("appofholding-{}", sub.get_name()))?;
            }
        }
        Cmd::Source(cmd) => source_cmd(cmd)?,
        cmd @ Cmd::Check { .. } => check_cmd(cmd)?,
        cmd @ Cmd::Update { .. } => update_cmd(cmd)?,
        Cmd::Versions { id, json: true } => {
            json_or_exit(library().and_then(|lib| pagejson::versions_json(&lib, &id)))
        }
        Cmd::Versions { id, json: false } => {
            let lib = library()?;
            let app = lib.get(&id)?;
            println!(
                "current  {:<14} {}",
                app.version.as_deref().unwrap_or("-"),
                &app.sha256[..12]
            );
            for v in lib.versions(&id)? {
                println!(
                    "kept     {:<14} {}",
                    v.version.as_deref().unwrap_or("-"),
                    &v.sha256[..12]
                );
            }
        }
        Cmd::Rollback {
            id,
            to,
            dry_run: true,
            json,
        } => {
            let plan = library().and_then(|lib| pagejson::rollback_plan(&lib, &id, to.as_deref()));
            if json {
                json_or_exit(plan);
            } else {
                println!("{}", plan?["summary"].as_str().unwrap_or(""));
            }
        }
        Cmd::Rollback {
            id,
            to,
            dry_run: false,
            json: true,
        } => json_or_exit(library().and_then(|lib| {
            let rec = lib.rollback(&id, to.as_deref())?;
            Ok(json!({ "version": 1, "ok": true, "id": id, "to": rec.version }))
        })),
        Cmd::Rollback {
            id,
            to,
            dry_run: false,
            json: false,
        } => {
            let rec = library()?.rollback(&id, to.as_deref())?;
            println!(
                "{id} is now version {}",
                rec.version.as_deref().unwrap_or("?")
            );
            println!(
                "The version you left is kept; the update you rolled back from will not be installed automatically."
            );
        }
        Cmd::Policy { id, policy } => {
            let Some(p) = Policy::parse(&policy) else {
                bail!("policy must be auto, notify, manual or pinned")
            };
            library()?.set_policy(&id, p)?;
            println!("{id}: update policy is now {}", p.as_str());
        }
        Cmd::Skip { id, clear } => {
            let lib = library()?;
            if clear {
                lib.clear_skip(&id)?;
                println!("{id}: no longer skipping any version");
            } else {
                println!("{id}: skipping {}", lib.skip_latest(&id)?);
            }
        }
        Cmd::Schedule(cmd) => schedule_cmd(cmd)?,
        Cmd::Defaults(cmd) => defaults_cmd(cmd)?,
        Cmd::Config(cmd) => config_cmd(cmd)?,
        Cmd::Launch(cmd) => launch_cmd(cmd)?,
        cmd @ Cmd::Run { .. } => run_app_cmd(cmd)?,
    }
    Ok(())
}

fn run_app_cmd(cmd: Cmd) -> Result<()> {
    let Cmd::Run {
        profile,
        print,
        unsandboxed,
        id,
        args,
    } = cmd
    else {
        unreachable!()
    };
    let lib = library()?;
    let plan = lib.plan_launch(&id, profile.as_deref(), &args, &launch_ctx(unsandboxed))?;
    if print {
        println!("{}", plan.display());
        for n in &plan.notes {
            eprintln!("note: {n}");
        }
        return Ok(());
    }
    // Unpack the app and create its private home on first sandboxed launch.
    if !unsandboxed {
        let nothing_to_unpack = !lib.sandbox_settings(&id)?.enabled()
            || appofholding_core::extract::is_extracted(&lib.dirs, &lib.get(&id)?.sha256);
        if !nothing_to_unpack {
            eprintln!("Unpacking {} for its first sandboxed launch...", id);
        }
        lib.prepare_launch(&id)?;
    }
    // Planning again after unpacking lets the sandbox see what kind of app this is.
    let plan = lib.plan_launch(&id, profile.as_deref(), &args, &launch_ctx(unsandboxed))?;
    for n in &plan.notes {
        eprintln!("note: {n}");
    }
    if plan.helper.is_some() {
        // A helper (the filtered D-Bus proxy) has to be supervised, so we cannot exec.
        std::process::exit(plan.run()?);
    }
    let err = plan.command().exec();
    bail!("failed to launch {}: {err}", plan.argv[0]);
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod json_tests;
