use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, bail};
use appimgr_core::assets::{self, Arch, Selection};
use appimgr_core::check::{self, Report};
use appimgr_core::engine::{Applied, ApplyOptions, IfRunning, Outcome};
use appimgr_core::http::UreqHttp;
use appimgr_core::notify::NotifySend;
use appimgr_core::schedule::{self, Interval};
use appimgr_core::source::Policy;
use appimgr_core::source::{self, Channel, Source};
use appimgr_core::version::{Installed, Status};
use appimgr_core::{AddOptions, Dirs, Install, Library, inspect};
use clap::{Parser, Subcommand};

/// Manage AppImages: integrate them into the desktop, track their update sources, and launch them.
#[derive(Parser)]
#[command(name = "appimgr", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Add an AppImage to the library and integrate it into the desktop menu.
    /// TARGET is a file, or a link to download from (GitHub/GitLab/Forgejo repo or release URL,
    /// `owner/repo`, or a direct URL).
    Add {
        target: String,
        /// Pick the release file with this pattern (`*` and `?` wildcards), e.g. 'Foo-*-x86_64.AppImage'.
        #[arg(long)]
        asset: Option<String>,
        /// Which release to follow: latest, latest-pre, latest-all or a tag name.
        #[arg(long)]
        channel: Option<String>,
        /// Move the file into the library instead of copying it.
        #[arg(long = "move")]
        move_file: bool,
        /// Use this id instead of one derived from the app name.
        #[arg(long)]
        id: Option<String>,
    },
    /// List apps in the library.
    List,
    /// Show details about an AppImage file or a library app id.
    Info { target: String },
    /// Remove an app: its menu entry, icon and (unless --keep-file) the AppImage.
    Remove {
        id: String,
        /// Keep the AppImage file on disk.
        #[arg(long)]
        keep_file: bool,
    },
    /// Inspect or change where an app gets its updates from.
    #[command(subcommand)]
    Source(SourceCmd),
    /// Check apps for updates (all apps with a source, or just ID).
    Check { id: Option<String> },
    /// Download and install newer versions (all apps with a source, or just ID).
    Update {
        id: Option<String>,
        /// What the background timer runs: follow each app's update policy, notify, never prompt.
        #[arg(long)]
        scheduled: bool,
        /// Ignore pinned/skipped state and update even if the status is only "unknown".
        #[arg(long)]
        force: bool,
        /// Leave the app alone (and say so) if it is currently running.
        #[arg(long)]
        defer_if_running: bool,
        /// How many previous versions to keep for rollback.
        #[arg(long, default_value_t = 2)]
        keep: usize,
    },
    /// List the previous versions kept for rollback.
    Versions { id: String },
    /// Go back to a kept version (the newest one unless --to is given).
    Rollback {
        id: String,
        /// Version string or SHA-256 prefix, as shown by `versions`.
        #[arg(long)]
        to: Option<String>,
    },
    /// Set what happens when an update is found: auto, notify, manual or pinned.
    Policy { id: String, policy: String },
    /// Ignore the latest known version of an app (until a newer one appears).
    Skip {
        id: String,
        /// Stop ignoring it.
        #[arg(long)]
        clear: bool,
    },
    /// Install or remove the systemd timer that checks for updates in the background.
    #[command(subcommand)]
    Schedule(ScheduleCmd),
    /// Launch an app (this is what generated menu entries call).
    Run {
        id: String,
        /// Arguments passed to the application.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

#[derive(Subcommand)]
enum ScheduleCmd {
    /// Enable background update checks.
    Install {
        /// hourly, daily or weekly
        #[arg(long, default_value = "daily")]
        interval: String,
    },
    /// Disable background update checks.
    Remove,
    /// Show whether the timer is installed and when it runs next.
    Status,
}

#[derive(Subcommand)]
enum SourceCmd {
    /// Show an app's update source and the result of its last check.
    Show { id: String },
    /// Set an app's update source.
    Set {
        id: String,
        /// Repo / release URL, `owner/repo`, direct URL or embedded update info.
        input: String,
        #[arg(long)]
        asset: Option<String>,
        #[arg(long)]
        channel: Option<String>,
    },
    /// Remove an app's update source.
    Clear { id: String },
    /// Try a source without saving it: shows the release and which file would be chosen.
    /// INPUT is an app id or anything `source set` accepts.
    Test {
        input: String,
        #[arg(long)]
        asset: Option<String>,
        #[arg(long)]
        channel: Option<String>,
    },
}

fn build_source(
    input: &str,
    asset: Option<String>,
    channel: Option<String>,
) -> Result<source::Parsed> {
    let mut parsed = source::parse_input(input)?;
    if let Some(a) = asset {
        parsed.source.asset_pattern = Some(a);
    }
    if let Some(c) = channel {
        parsed.source.channel = Channel::parse(&c);
    }
    Ok(parsed)
}

fn print_report(source: &Source, r: &Report) {
    println!("source:  {}", source.describe());
    if let Some(p) = &source.asset_pattern {
        println!("pattern: {p}");
    }
    print!("latest:  {}", r.release.tag);
    if r.release.prerelease {
        print!(" (pre-release)");
    }
    if let Some(d) = &r.release.published_at {
        print!("  published {d}");
    }
    println!();
    println!("assets:");
    for a in &r.release.assets {
        let mark = if r.asset().is_some_and(|x| x.name == a.name) {
            "->"
        } else if matches!(&r.selection, Selection::Ambiguous(v) if v.iter().any(|x| x.name == a.name))
        {
            "? "
        } else {
            "  "
        };
        let sha = if a.sha256.is_some() { "  sha256" } else { "" };
        println!("  {mark} {}{sha}", a.name);
    }
    match r.problem() {
        Some(p) => println!("problem: {p}"),
        None => println!("status:  {}", r.status.as_str()),
    }
}

fn progress() -> impl FnMut(u64, Option<u64>) {
    let mut last = 0u64;
    move |done, total| {
        if done - last >= 512 * 1024 || Some(done) == total {
            last = done;
            match total {
                Some(t) if t > 0 => eprint!(
                    "\rDownloading {:>3}% ({} / {} MiB)",
                    done * 100 / t,
                    done >> 20,
                    t >> 20
                ),
                _ => eprint!("\rDownloading {} MiB", done >> 20),
            }
            if Some(done) == total {
                eprintln!();
            }
        }
    }
}

fn describe_outcome(o: &Outcome) -> String {
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

fn systemctl(args: &[&str]) -> bool {
    Command::new("systemctl")
        .arg("--user")
        .args(args)
        .status()
        .is_ok_and(|s| s.success())
}

fn schedule_cmd(cmd: ScheduleCmd) -> Result<()> {
    let dirs = dirs()?;
    match cmd {
        ScheduleCmd::Install { interval } => {
            let Some(iv) = Interval::parse(&interval) else {
                bail!("interval must be hourly, daily or weekly")
            };
            let exe = std::env::current_exe().context("cannot determine own path")?;
            let (svc, timer) = schedule::write_units(&dirs, &exe, iv)?;
            println!("Wrote {}\n      {}", svc.display(), timer.display());
            if dirs.refresh_caches {
                if systemctl(&["daemon-reload"])
                    && systemctl(&["enable", "--now", "appimgr-update.timer"])
                {
                    println!("Background checks are on ({interval}).");
                } else {
                    println!(
                        "Could not enable the timer; run: systemctl --user enable --now appimgr-update.timer"
                    );
                }
            }
        }
        ScheduleCmd::Remove => {
            if dirs.refresh_caches {
                systemctl(&["disable", "--now", "appimgr-update.timer"]);
            }
            let removed = schedule::remove_units(&dirs)?;
            if dirs.refresh_caches {
                systemctl(&["daemon-reload"]);
            }
            println!(
                "{}",
                if removed {
                    "Background checks are off."
                } else {
                    "Background checks were not installed."
                }
            );
        }
        ScheduleCmd::Status => {
            let (svc, timer) = schedule::unit_paths(&dirs);
            println!(
                "units:  {}",
                if svc.exists() && timer.exists() {
                    "installed"
                } else {
                    "not installed"
                }
            );
            if dirs.refresh_caches {
                systemctl(&["list-timers", "appimgr-update.timer", "--no-pager"]);
            }
        }
    }
    Ok(())
}

fn source_cmd(cmd: SourceCmd) -> Result<()> {
    match cmd {
        SourceCmd::Show { id } => {
            let lib = library()?;
            let Some(s) = lib.source(&id)? else {
                println!("{id} has no update source.");
                return Ok(());
            };
            println!("source:    {}", s.source.describe());
            println!(
                "pattern:   {}",
                s.source
                    .asset_pattern
                    .as_deref()
                    .unwrap_or("(any AppImage for this architecture)")
            );
            println!(
                "origin:    {}",
                if s.source.origin == source::Origin::Embedded {
                    "embedded in the AppImage"
                } else {
                    "set by you"
                }
            );
            println!(
                "installed: {}",
                s.installed_version.as_deref().unwrap_or("-")
            );
            println!(
                "checked:   {}",
                s.last_checked
                    .map_or("never".into(), |t| format!("unix {t}"))
            );
            println!("latest:    {}", s.last_latest.as_deref().unwrap_or("-"));
            println!("status:    {}", s.last_status.as_deref().unwrap_or("-"));
            if let Some(e) = &s.last_error {
                println!("last error: {e}");
            }
        }
        SourceCmd::Set {
            id,
            input,
            asset,
            channel,
        } => {
            let lib = library()?;
            let parsed = build_source(&input, asset, channel)?;
            lib.set_source(&id, &parsed.source)?;
            println!("{id} now updates from {}", parsed.source.describe());
            println!("Run `appimgr source test {id}` to verify it.");
        }
        SourceCmd::Clear { id } => {
            let removed = library()?.clear_source(&id)?;
            println!(
                "{}",
                if removed {
                    "Source removed."
                } else {
                    "Nothing to remove."
                }
            );
        }
        SourceCmd::Test {
            input,
            asset,
            channel,
        } => {
            let lib = library()?;
            let known = lib.source(&input).ok().flatten();
            let (mut source, version, digest) = match known {
                Some(rec) => {
                    let app = lib.get(&input)?;
                    (
                        rec.source,
                        rec.installed_version.or(app.version),
                        rec.installed_digest,
                    )
                }
                None => (build_source(&input, None, None)?.source, None, None),
            };
            if let Some(a) = asset {
                source.asset_pattern = Some(a);
            }
            if let Some(c) = channel {
                source.channel = Channel::parse(&c);
            }
            let installed = Installed {
                version: version.as_deref(),
                sha256: digest.as_deref(),
            };
            let r = check::check(&UreqHttp::new(), &source, &installed, Arch::host())?;
            print_report(&source, &r);
            if r.asset().is_none()
                && source.asset_pattern.is_none()
                && let Selection::Ambiguous(v) = &r.selection
                && let Some(first) = v.first()
            {
                println!(
                    "hint:    e.g. --asset '{}'",
                    assets::suggest_pattern(&first.name)
                );
            }
        }
    }
    Ok(())
}

fn dirs() -> Result<Dirs> {
    // APPIMGR_ROOT redirects everything into one directory (used by tests and for experiments).
    match std::env::var_os("APPIMGR_ROOT") {
        Some(root) => Ok(Dirs::with_root(&PathBuf::from(root))),
        None => Ok(Dirs::from_env()?),
    }
}

fn library() -> Result<Library> {
    let exe = std::env::current_exe().context("cannot determine own path")?;
    Ok(Library::open(dirs()?, exe)?)
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Cmd::Add {
            target,
            asset,
            channel,
            move_file,
            id,
        } => {
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
            let rec = if path.is_file() {
                lib.add(&path, &opts)?
            } else {
                let parsed = build_source(&target, asset, channel)?;
                println!("Resolving {} ...", parsed.source.describe());
                let (rec, report) =
                    lib.install_from_source(&UreqHttp::new(), parsed, &opts, &mut progress())?;
                println!(
                    "Downloaded {} ({})",
                    report.asset().map_or("?", |a| a.name.as_str()),
                    report.release.tag
                );
                rec
            };
            println!("Added {} ({})", rec.name, rec.id);
            println!("  file: {}", rec.appimage_path.display());
            println!("  menu: {}", rec.desktop_file.display());
            if let Some(s) = lib.source(&rec.id)? {
                println!("  updates: {}", s.source.describe());
            }
        }
        Cmd::List => {
            let apps = library()?.list()?;
            if apps.is_empty() {
                println!("No apps yet. Add one with: appimgr add <file>");
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
        Cmd::Info { target } => {
            let path = PathBuf::from(&target);
            if path.is_file() {
                let i = inspect(&path)?;
                println!("name:        {}", i.name);
                println!("id:          {}", i.id);
                println!("version:     {}", i.version.as_deref().unwrap_or("-"));
                println!("type:        {}", i.image.kind);
                println!("desktop:     {}", i.desktop_path);
                println!(
                    "update info: {}",
                    i.image.update_info.as_deref().unwrap_or("-")
                );
                println!(
                    "icon:        {}",
                    match &i.icon {
                        Some(ic) => format!(
                            "{:?} {}",
                            ic.format,
                            ic.size.map_or("scalable".into(), |s| format!("{s}px"))
                        ),
                        None => "none".into(),
                    }
                );
            } else {
                let a = library()?.get(&target)?;
                println!("name:        {}", a.name);
                println!("id:          {}", a.id);
                println!("version:     {}", a.version.as_deref().unwrap_or("-"));
                println!("file:        {}", a.appimage_path.display());
                println!("sha256:      {}", a.sha256);
                println!("size:        {} bytes", a.size);
                println!("update info: {}", a.update_info.as_deref().unwrap_or("-"));
            }
        }
        Cmd::Remove { id, keep_file } => {
            let rec = library()?.remove(&id, keep_file)?;
            println!("Removed {} ({})", rec.name, rec.id);
        }
        Cmd::Source(cmd) => source_cmd(cmd)?,
        Cmd::Check { id } => {
            let lib = library()?;
            let http = UreqHttp::new();
            let ids: Vec<String> = match id {
                Some(i) => vec![i],
                None => lib.sources()?.into_iter().map(|s| s.app_id).collect(),
            };
            if ids.is_empty() {
                println!(
                    "No apps have an update source yet. Use `appimgr source set <id> <link>`."
                );
            }
            for i in ids {
                match lib.check(&http, &i) {
                    Ok(r) => match r.problem() {
                        Some(p) => println!("{i:<24} problem: {p}"),
                        None => println!(
                            "{i:<24} {:<16} latest {}",
                            r.status.as_str(),
                            r.latest_version()
                        ),
                    },
                    Err(e) => println!("{i:<24} error: {e}"),
                }
            }
        }
        Cmd::Update {
            id,
            scheduled,
            force,
            defer_if_running,
            keep,
        } => {
            let lib = library()?;
            let http = UreqHttp::new();
            if scheduled {
                for o in lib.run_scheduled(&http, &NotifySend)? {
                    println!("{}", describe_outcome(&o));
                }
                return Ok(());
            }
            let opts = ApplyOptions {
                if_running: if defer_if_running {
                    IfRunning::Defer
                } else {
                    IfRunning::Proceed
                },
                keep_versions: keep,
                force,
            };
            let ids: Vec<String> = match id {
                Some(i) => vec![i],
                None => lib.sources()?.into_iter().map(|s| s.app_id).collect(),
            };
            if ids.is_empty() {
                println!(
                    "No apps have an update source yet. Use `appimgr source set <id> <link>`."
                );
            }
            let mut failed = false;
            for i in ids {
                match lib.update(&http, &i, &opts, &mut progress()) {
                    Ok(Applied::Updated { from, to, delta }) => println!(
                        "{i}: updated {} -> {to}{}",
                        from.as_deref().unwrap_or("?"),
                        if delta { " (delta)" } else { "" }
                    ),
                    Ok(Applied::NoUpdate(Status::Unknown)) => println!(
                        "{i}: can't tell if there is an update (use --force to reinstall the latest)"
                    ),
                    Ok(Applied::NoUpdate(_)) => println!("{i}: already up to date"),
                    Ok(Applied::Skipped(why)) => println!("{i}: skipped ({why})"),
                    Ok(Applied::Deferred { version, pids }) => println!(
                        "{i}: {version} is available but the app is running (pid {}); try again after closing it",
                        pids.iter()
                            .map(u32::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    Err(e) => {
                        failed = true;
                        println!("{i}: error: {e}");
                    }
                }
            }
            if failed {
                bail!("some updates failed");
            }
        }
        Cmd::Versions { id } => {
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
        Cmd::Rollback { id, to } => {
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
        Cmd::Run { id, args } => {
            let rec = library()?.get(&id)?;
            // M4 will add launch profiles and sandboxing here; for now just exec the AppImage.
            let err = Command::new(&rec.appimage_path).args(args).exec();
            bail!("failed to launch {}: {err}", rec.appimage_path.display());
        }
    }
    Ok(())
}
