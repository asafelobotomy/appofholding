use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use appofholding_core::assets::{self, Arch, Selection};
use appofholding_core::check::{self, Report};
use appofholding_core::config::{ApplyOptions as ConfigApply, Config, Note};
use appofholding_core::desktop;
use appofholding_core::engine::{Applied, ApplyOptions, IfRunning, Outcome};
use appofholding_core::http::UreqHttp;
use appofholding_core::import;
use appofholding_core::launch;
use appofholding_core::notify::NotifySend;
use appofholding_core::profiles::{self, LaunchCtx};
use appofholding_core::sandbox::{self, Bus, HomeAccess, Level, PathGrant};
use appofholding_core::schedule::{self, Interval};
use appofholding_core::selfupdate;
use appofholding_core::source::Policy;
use appofholding_core::source::{self, Channel, Source};
use appofholding_core::version::{Installed, Status};
use appofholding_core::{AddOptions, Dirs, FilePolicy, Install, Library, inspect};
use clap::{CommandFactory, Parser, Subcommand};

/// Manage AppImages: integrate them into the desktop, track their update sources, and launch them.
#[derive(Parser)]
#[command(name = "appofholding", version, about)]
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
        /// Shell command to run once after adding (also kept as the app's post-install hook).
        #[arg(long)]
        post_install: Option<String>,
    },
    /// List apps in the library.
    List,
    /// Show details about an AppImage file or a library app id.
    Info { target: String },
    /// Remove an app: its menu entry, icon and the AppImage (but see --keep-file; an AppImage that
    /// was imported in place is your file and is kept unless you add --delete-file).
    Remove {
        id: String,
        /// Keep the AppImage file on disk.
        #[arg(long, conflicts_with = "delete_file")]
        keep_file: bool,
        /// Delete the AppImage file even if it was imported in place.
        #[arg(long)]
        delete_file: bool,
        /// Also delete the app's private sandbox home (its settings and files). Cannot be undone.
        #[arg(long)]
        purge_data: bool,
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
    /// Check for a new release of appOFholding itself, and update it where that is possible.
    #[command(subcommand, name = "self")]
    SelfUpdate(SelfCmd),
    /// Write everything you have set up (sources, policies, launch profiles, sandboxes, hooks)
    /// to a portable TOML file.
    Export {
        /// Write to this file instead of the terminal.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Show simple improvements for apps (Wayland for Electron apps, Chromium's own sandbox,
    /// starting without FUSE 2, space used by old versions), and apply or dismiss one.
    Suggest {
        /// The app; without it, every app is checked.
        id: Option<String>,
        /// Apply the suggestion with this key (shown in brackets).
        #[arg(long, requires = "id", conflicts_with = "dismiss")]
        apply: Option<String>,
        /// Stop showing the suggestion with this key.
        #[arg(long, requires = "id")]
        dismiss: Option<String>,
    },
    /// Make the library match a configuration file (see `export`). Safe to run repeatedly.
    Apply {
        file: PathBuf,
        /// Do not download apps that are missing.
        #[arg(long)]
        no_install: bool,
        /// Also remove extra launch profiles, hooks and sandboxes the file does not mention.
        #[arg(long)]
        prune: bool,
        /// Only show what would be done.
        #[arg(long)]
        dry_run: bool,
        /// Also set up hooks, command prefixes and run-as-root profiles from the file. Without
        /// this, entries that contain them are refused. Read the file first.
        #[arg(long)]
        allow_commands: bool,
    },
    /// Print a shell completion script (bash, zsh, fish, elvish, powershell).
    Completions { shell: clap_complete::Shell },
    /// Write man pages into a directory.
    #[command(hide = true)]
    Man { dir: PathBuf },
    /// Find AppImages already on this computer and import them. Shows the whole plan first
    /// (nothing changes until you confirm).
    Import(ImportArgs),
    /// Run an app inside a bubblewrap sandbox with limited access to your files, network, etc.
    #[command(subcommand)]
    Sandbox(SandboxCmd),
    /// View and change how an app is launched: arguments, environment, presets, hooks.
    #[command(subcommand)]
    Launch(LaunchCmd),
    /// Launch an app (this is what generated menu entries call). Options must come before ID;
    /// everything after ID is passed to the application.
    Run {
        /// Use this launch profile instead of the default one.
        #[arg(long)]
        profile: Option<String>,
        /// Print the command instead of running it.
        #[arg(long)]
        print: bool,
        /// Run without the sandbox this once (for debugging).
        #[arg(long)]
        unsandboxed: bool,
        id: String,
        /// Arguments passed to the application.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

#[derive(Subcommand)]
enum SandboxCmd {
    /// Show an app's sandbox settings.
    Show { id: String },
    /// Change sandbox settings. Only the options you give are changed; `--level` starts from a preset.
    Set {
        id: String,
        /// none, standard or strict (resets everything except shared folders to that preset).
        #[arg(long)]
        level: Option<String>,
        #[arg(long)]
        network: Option<bool>,
        /// private (own empty home), ro (your home, read-only) or rw (your home, writable).
        #[arg(long)]
        home: Option<String>,
        #[arg(long)]
        gpu: Option<bool>,
        #[arg(long)]
        audio: Option<bool>,
        /// Cameras and other media devices.
        #[arg(long)]
        devices: Option<bool>,
        /// Wayland / X11 access (off = the app cannot open windows).
        #[arg(long)]
        display: Option<bool>,
        /// off, filtered (portals, notifications, tray) or full.
        #[arg(long)]
        bus: Option<String>,
        /// Share a folder: PATH (read-only) or PATH:rw (writable). Repeat; adds to the list.
        #[arg(long)]
        allow: Vec<String>,
        /// Forget all shared folders first.
        #[arg(long)]
        clear_allow: bool,
        /// Add --no-sandbox for Chromium/Electron apps (needed inside this sandbox).
        #[arg(long)]
        chromium_workaround: Option<bool>,
    },
    /// Unpack an app now instead of at its first sandboxed launch.
    Prepare { id: String },
    /// Delete unpacked copies nothing needs any more.
    Gc,
    /// Check that this machine can run sandboxes (bubblewrap, user namespaces, D-Bus proxy).
    Check,
}

#[derive(clap::Args)]
#[command(args_conflicts_with_subcommands = true)]
struct ImportArgs {
    #[command(subcommand)]
    action: Option<ImportAction>,
    /// Also look in this folder (repeatable).
    #[arg(long = "dir")]
    dirs: Vec<PathBuf>,
    /// Look only in the --dir folders, not the usual places.
    #[arg(long)]
    only_dirs: bool,
    /// How to bring in loose and tool-integrated files: in-place (leave them), copy, or move
    /// them into the library folder. Default: leave them, except files in Downloads/Desktop
    /// which are moved. AppImages an app manages itself always stay where they are.
    #[arg(long)]
    mode: Option<String>,
    /// Back up the menu entries other tools (Gear Lever, AppImageLauncher...) made for these
    /// AppImages, so each app appears once. Undo with `appofholding import restore`.
    #[arg(long)]
    replace_entries: bool,
    /// Import only these (names, ids or the numbers shown in the plan), comma separated.
    #[arg(long, value_delimiter = ',')]
    only: Vec<String>,
    /// Leave these out (names, ids or numbers).
    #[arg(long, value_delimiter = ',')]
    exclude: Vec<String>,
    /// Also import the ones needing a decision, under the suggested different id.
    #[arg(long)]
    include_conflicts: bool,
    /// Show the plan and stop; change nothing.
    #[arg(long)]
    dry_run: bool,
    /// Do not ask for confirmation.
    #[arg(short, long)]
    yes: bool,
}

#[derive(Subcommand)]
enum ImportAction {
    /// Put back menu entries that `import --replace-entries` backed up.
    Restore {
        /// Which backup (see --list); default: the newest.
        batch: Option<String>,
        /// Only list the backups.
        #[arg(long)]
        list: bool,
    },
}

/// What the user answered at the confirmation prompt.
#[derive(Debug, PartialEq, Eq)]
enum Answer {
    No,
    /// Everything that is currently selected.
    Selected,
    /// Exactly these plan numbers (1-based).
    Numbers(Vec<usize>),
}

fn parse_answer(text: &str, count: usize) -> Answer {
    let t = text.trim().to_lowercase();
    if t == "y" || t == "yes" {
        return Answer::Selected;
    }
    if t.is_empty() || t == "n" || t == "no" {
        return Answer::No;
    }
    let nums: Option<Vec<usize>> = t
        .split([',', ' '])
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<usize>().ok().filter(|n| (1..=count).contains(n)))
        .collect();
    match nums {
        Some(v) if !v.is_empty() => Answer::Numbers(v),
        _ => Answer::No,
    }
}

fn short(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

fn import_cmd(args: ImportArgs) -> Result<()> {
    let lib = library()?;
    let home = PathBuf::from(std::env::var_os("HOME").context("$HOME is not set")?);
    if let Some(ImportAction::Restore { batch, list }) = args.action {
        let backups = import::list_backups(&lib);
        if backups.is_empty() {
            println!("No menu entries are backed up.");
            return Ok(());
        }
        if list {
            for b in &backups {
                println!(
                    "{}: {}",
                    b.batch,
                    b.files
                        .iter()
                        .map(|f| short(f, &home))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            return Ok(());
        }
        let batch = batch.unwrap_or_else(|| backups[0].batch.clone());
        let restored = import::restore(&lib, &batch)?;
        for r in &restored {
            println!("Restored {}", short(r, &home));
        }
        if restored.is_empty() {
            println!(
                "Nothing restored (the original files are in the way, or the backup is gone)."
            );
        }
        return Ok(());
    }

    let mode = match args.mode.as_deref() {
        None => None,
        Some("in-place" | "inplace") => Some(Install::InPlace),
        Some("copy") => Some(Install::Copy),
        Some("move") => Some(Install::Move),
        Some(other) => bail!("--mode must be in-place, copy or move (not '{other}')"),
    };
    let scan_opts = import::ScanOptions {
        extra_dirs: args.dirs.clone(),
        only_extra: args.only_dirs,
        ..import::ScanOptions::new(&home)
    };
    println!("Looking for AppImages...");
    let scan = import::scan(&lib, &scan_opts)?;
    let plan = import::plan(
        &lib,
        &scan,
        &home,
        &import::PlanOptions {
            mode,
            replace_entries: args.replace_entries,
        },
    )?;
    println!(
        "Searched: {}",
        scan.searched
            .iter()
            .map(|d| short(d, &home))
            .collect::<Vec<_>>()
            .join(", ")
    );

    // Which items are selected by default / by the filters.
    let matches = |item: &import::Item, n: usize, list: &[String]| {
        list.iter().any(|w| {
            let w = w.trim().to_lowercase();
            w == item.id
                || w == item.found.name.to_lowercase()
                || w.parse::<usize>().ok() == Some(n)
        })
    };
    let mut selected: Vec<usize> = Vec::new();
    for (i, item) in plan.items.iter().enumerate() {
        let n = i + 1;
        let base = match &item.action {
            import::Action::Import => true,
            import::Action::Conflict { .. } => args.include_conflicts,
            import::Action::Skip(_) => false,
        };
        let wanted = if args.only.is_empty() {
            base
        } else {
            matches(item, n, &args.only)
        };
        if wanted
            && !matches(item, n, &args.exclude)
            && !matches!(item.action, import::Action::Skip(_))
        {
            selected.push(i);
        }
    }

    println!();
    if plan.items.is_empty() {
        println!("No importable AppImages found.");
    } else {
        println!("Found {} AppImage(s):\n", plan.items.len());
    }
    for (i, item) in plan.items.iter().enumerate() {
        let f = &item.found;
        let mark = if selected.contains(&i) { "[x]" } else { "[ ]" };
        let owner = match &f.owner {
            import::Owner::Loose => "loose file".to_string(),
            import::Owner::Tool { tool, .. } => format!("integrated by {tool}"),
            import::Owner::Entry { .. } => "has its own menu entry".to_string(),
            import::Owner::App { .. } => "kept by the app itself".to_string(),
        };
        println!(
            "{:>3} {mark} {}{}  ({}, {})",
            i + 1,
            f.name,
            f.version
                .as_deref()
                .map(|v| format!(" {v}"))
                .unwrap_or_default(),
            owner,
            Utf8Size(f.size)
        );
        println!("        {}", short(&f.path, &home));
        match &item.action {
            import::Action::Import => {}
            import::Action::Conflict { reason, alt_id } => println!(
                "        needs a decision: {reason}; --include-conflicts imports it as '{alt_id}'"
            ),
            import::Action::Skip(why) => println!("        not importable: {why}"),
        }
        for n in &item.notes {
            println!("        - {n}");
        }
    }
    if !scan.skipped.is_empty() {
        println!("\nNot offered ({}):", scan.skipped.len());
        for (p, why) in &scan.skipped {
            println!("  {}  -- {why}", short(p, &home));
        }
    }
    println!();
    if selected.is_empty() {
        println!("Nothing selected to import.");
        return Ok(());
    }
    println!(
        "{} of {} selected for import.",
        selected.len(),
        plan.items.len()
    );
    if args.dry_run {
        println!("Dry run: nothing was changed.");
        return Ok(());
    }

    use std::io::{BufRead, IsTerminal, Write};
    if !args.yes {
        if !std::io::stdin().is_terminal() {
            println!(
                "Nothing was changed. Re-run with --yes to import the selected apps (or --dry-run to only look)."
            );
            return Ok(());
        }
        print!(
            "Import the {} selected app(s)? [y/N, or numbers like 1,3 to choose] ",
            selected.len()
        );
        std::io::stdout().flush().ok();
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line)?;
        match parse_answer(&line, plan.items.len()) {
            Answer::No => {
                println!("Nothing was changed.");
                return Ok(());
            }
            Answer::Selected => {}
            Answer::Numbers(nums) => {
                selected = nums
                    .into_iter()
                    .map(|n| n - 1)
                    .filter(|i| !matches!(plan.items[*i].action, import::Action::Skip(_)))
                    .collect();
            }
        }
    }

    println!();
    let mut failed = 0;
    let results = import::execute(&lib, &plan, &selected);
    for r in &results {
        match &r.outcome {
            Ok(notes) => println!("OK     {}  ({})", r.id, notes.join("; ")),
            Err(e) => {
                failed += 1;
                println!("FAILED {}  {}", short(&r.path, &home), e);
            }
        }
    }
    println!(
        "\nImported {} of {}.",
        results.len() - failed,
        results.len()
    );
    if plan.items.iter().enumerate().any(|(i, it)| {
        selected.contains(&i)
            && matches!(it.found.owner, import::Owner::Tool { .. })
            && it.replace_entries.is_empty()
    }) {
        println!(
            "Some apps now have two menu entries (yours and the other tool's). Run again with --replace-entries to back up the other tool's entries."
        );
    }
    print_warnings(&lib);
    if failed > 0 {
        bail!("{failed} app(s) could not be imported");
    }
    Ok(())
}

struct Utf8Size(u64);

impl std::fmt::Display for Utf8Size {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mb = self.0 as f64 / (1024.0 * 1024.0);
        if mb >= 1.0 {
            write!(f, "{mb:.1} MiB")
        } else {
            write!(f, "{} KiB", self.0 / 1024)
        }
    }
}

#[derive(Subcommand)]
enum SelfCmd {
    /// Is there a newer release?
    Check {
        /// Ask GitHub even if a recent answer is cached.
        #[arg(long)]
        force: bool,
    },
    /// Update appOFholding. Package installs download a verified package and show (or, with
    /// --install, run) the command; AppImage installs replace themselves; others get instructions.
    Update {
        /// For package installs: run `run0 pacman -U` on the downloaded package.
        #[arg(long)]
        install: bool,
    },
}

fn self_cmd(cmd: SelfCmd) -> Result<()> {
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

#[derive(Subcommand)]
enum LaunchCmd {
    /// List the built-in presets (e.g. electron-wayland, x11, no-sandbox).
    Presets,
    /// List an app's launch profiles.
    Profiles { id: String },
    /// Show a profile and the exact command it produces.
    Show {
        id: String,
        #[arg(long)]
        profile: Option<String>,
    },
    /// Change a profile. Only the options you give are changed.
    Set {
        id: String,
        /// Profile name (default: the profile used by the menu entry). Other names appear in the
        /// app's right-click menu.
        #[arg(long)]
        profile: Option<String>,
        /// Extra arguments, quoted as one string: --args '--foo "a b"'
        #[arg(long, allow_hyphen_values = true)]
        args: Option<String>,
        /// Environment variable NAME=value (repeat; replaces the existing set).
        #[arg(long)]
        env: Vec<String>,
        /// Command to run the app under, e.g. --prefix 'gamemoderun'
        #[arg(long, allow_hyphen_values = true)]
        prefix: Option<String>,
        /// Working directory.
        #[arg(long)]
        cwd: Option<String>,
        #[arg(long)]
        clear_cwd: bool,
        /// Preset to apply (repeat; replaces the existing set). See `launch presets`.
        #[arg(long)]
        preset: Vec<String>,
        #[arg(long)]
        clear_presets: bool,
        /// Unpack and run instead of mounting (no FUSE needed).
        #[arg(long)]
        extract_and_run: Option<bool>,
        /// Start as root via run0/pkexec. Off by default.
        #[arg(long)]
        elevated: Option<bool>,
        /// Start from an empty profile before applying the options.
        #[arg(long)]
        reset: bool,
    },
    /// Delete a profile (deleting `default` clears its settings).
    Delete {
        id: String,
        #[arg(long)]
        profile: String,
    },
    /// Commands that run at install/update time.
    #[command(subcommand)]
    Hook(HookCmd),
}

#[derive(Subcommand)]
enum HookCmd {
    Show {
        id: String,
    },
    /// KIND is post-install, pre-update or post-update; a failing pre-update hook cancels the update.
    Set {
        id: String,
        kind: String,
        command: String,
    },
    Clear {
        id: String,
        kind: String,
    },
    /// Run a hook now.
    Run {
        id: String,
        kind: String,
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

fn launch_ctx(unsandboxed: bool) -> LaunchCtx {
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

fn onoff(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

fn on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
}

fn sandbox_cmd(cmd: SandboxCmd) -> Result<()> {
    match cmd {
        SandboxCmd::Check => {
            let bwrap = on_path("bwrap");
            println!(
                "bubblewrap (bwrap):   {}",
                if bwrap {
                    "found"
                } else {
                    "MISSING - install the 'bubblewrap' package"
                }
            );
            let works = bwrap
                && Command::new("bwrap")
                    .args(appofholding_core::sandbox::probe_args())
                    .output()
                    .is_ok_and(|o| o.status.success());
            println!(
                "sandboxes start:      {}",
                if works {
                    "yes (user namespaces work)"
                } else {
                    "NO - bwrap could not create a sandbox (user namespaces disabled?)"
                }
            );
            println!(
                "xdg-dbus-proxy:       {}",
                if on_path("xdg-dbus-proxy") {
                    "found (filtered session bus available)"
                } else {
                    "missing (use `--bus off` or `--bus full`)"
                }
            );
            println!(
                "wayland / x11:        {} / {}",
                std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "-".into()),
                std::env::var("DISPLAY").unwrap_or_else(|_| "-".into())
            );
        }
        SandboxCmd::Gc => {
            let n = library()?.gc_extracted()?;
            println!(
                "Removed {n} unpacked cop{}.",
                if n == 1 { "y" } else { "ies" }
            );
        }
        SandboxCmd::Prepare { id } => {
            let lib = library()?;
            if !lib.sandbox_settings(&id)?.enabled() {
                bail!(
                    "{id} has no sandbox; turn it on first: appofholding sandbox set {id} --level standard"
                );
            }
            let t = std::time::Instant::now();
            lib.prepare_launch(&id)?;
            println!(
                "{id} is unpacked and ready ({:.1}s).",
                t.elapsed().as_secs_f32()
            );
        }
        SandboxCmd::Show { id } => {
            let lib = library()?;
            let s = lib.sandbox_settings(&id)?;
            println!("level:    {}", s.level.as_str());
            if !s.enabled() {
                println!(
                    "(the app runs without a sandbox; enable one with `appofholding sandbox set {id} --level standard`)"
                );
                return Ok(());
            }
            println!("network:  {}", onoff(s.network));
            println!(
                "home:     {}",
                match s.home {
                    HomeAccess::Private =>
                        format!("private ({})", lib.dirs.private_home(&id).display()),
                    HomeAccess::ReadOnly => "your home, read-only".into(),
                    HomeAccess::ReadWrite => "your home, writable".into(),
                }
            );
            println!("display:  {}", onoff(s.display));
            println!("gpu:      {}", onoff(s.gpu));
            println!("audio:    {}", onoff(s.audio));
            println!("devices:  {}", onoff(s.devices));
            println!("bus:      {}", s.bus.as_str());
            println!(
                "chromium: {}",
                if s.chromium_workaround {
                    "--no-sandbox added automatically"
                } else {
                    "no workaround"
                }
            );
            if s.paths.is_empty() {
                println!("shared folders: none");
            }
            for g in &s.paths {
                println!(
                    "shared:   {} ({})",
                    g.path.display(),
                    if g.write { "read-write" } else { "read-only" }
                );
            }
        }
        SandboxCmd::Set {
            id,
            level,
            network,
            home,
            gpu,
            audio,
            devices,
            display,
            bus,
            allow,
            clear_allow,
            chromium_workaround,
        } => {
            let lib = library()?;
            let mut s = lib.sandbox_settings(&id)?;
            if let Some(l) = level {
                let Some(l) = Level::parse(&l) else {
                    bail!("level must be none, standard or strict")
                };
                if l == Level::Custom {
                    bail!("'custom' is what you get by changing individual options");
                }
                s = sandbox::Settings {
                    paths: s.paths,
                    ..sandbox::Settings::for_level(l)
                };
            } else if !s.enabled() {
                // Changing options on an app with no sandbox starts from the standard preset.
                s = sandbox::Settings {
                    paths: s.paths,
                    ..sandbox::Settings::for_level(Level::Standard)
                };
            }
            if let Some(v) = network {
                s.network = v;
            }
            if let Some(v) = gpu {
                s.gpu = v;
            }
            if let Some(v) = audio {
                s.audio = v;
            }
            if let Some(v) = devices {
                s.devices = v;
            }
            if let Some(v) = display {
                s.display = v;
            }
            if let Some(v) = chromium_workaround {
                s.chromium_workaround = v;
            }
            if let Some(h) = home {
                s.home = HomeAccess::parse(&h).context("home must be private, ro or rw")?;
            }
            if let Some(b) = bus {
                s.bus = Bus::parse(&b).context("bus must be off, filtered or full")?;
            }
            if clear_allow {
                s.paths.clear();
            }
            for a in allow {
                let (p, write) = match a.rsplit_once(':') {
                    Some((p, "rw")) => (p, true),
                    Some((p, "ro")) => (p, false),
                    _ => (a.as_str(), false),
                };
                let path = PathBuf::from(p);
                if !s.paths.iter().any(|g| g.path == path) {
                    s.paths.push(PathGrant { path, write });
                } else {
                    s.paths
                        .iter_mut()
                        .filter(|g| g.path == path)
                        .for_each(|g| g.write = write);
                }
            }
            let saved = lib.set_sandbox(&id, &s)?;
            println!("{id}: sandbox level is now '{}'", saved.level.as_str());
            if saved.enabled() {
                if saved.home == HomeAccess::ReadWrite {
                    println!(
                        "note: with writable home access the app can change any of your files"
                    );
                }
                println!("Preview the command with: appofholding run --print {id}");
            }
        }
    }
    Ok(())
}

fn print_warnings(lib: &Library) {
    for w in lib.take_warnings() {
        eprintln!("warning: {w}");
    }
}

fn kv(s: &str) -> Result<(String, String)> {
    s.split_once('=')
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .with_context(|| format!("'{s}' should look like NAME=value"))
}

fn launch_cmd(cmd: LaunchCmd) -> Result<()> {
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
                lib.profile(&id, &name).unwrap_or_default()
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

fn hook_cmd(cmd: HookCmd) -> Result<()> {
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

fn schedule_cmd(cmd: ScheduleCmd) -> Result<()> {
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
        ScheduleCmd::Status => {
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
            println!("Run `appofholding source test {id}` to verify it.");
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
    Ok(Dirs::detect()?)
}

fn library() -> Result<Library> {
    let exe = std::env::current_exe().context("cannot determine own path")?;
    Ok(Library::open(
        dirs()?,
        appofholding_core::paths::stable_exec_path(exe),
    )?)
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Cmd::Add {
            target,
            asset,
            channel,
            move_file,
            id,
            post_install,
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
            if let Some(cmd) = post_install {
                lib.set_hook(&rec.id, "post-install", &cmd)?;
                lib.run_hook(&rec.id, "post-install")?;
                println!("  post-install hook ran");
            }
        }
        Cmd::Suggest { id, apply, dismiss } => {
            let lib = library()?;
            if let (Some(id), Some(key)) = (&id, &apply) {
                println!("{id}: {}", lib.apply_suggestion(id, key)?);
                return Ok(());
            }
            if let (Some(id), Some(key)) = (&id, &dismiss) {
                lib.dismiss_suggestion(id, key)?;
                println!("{id}: '{key}' will not be suggested again.");
                return Ok(());
            }
            let host = appofholding_core::suggest::Host::detect();
            let ids: Vec<String> = match id {
                Some(i) => vec![i],
                None => lib.list()?.into_iter().map(|a| a.id).collect(),
            };
            let mut any = false;
            for i in &ids {
                match lib.suggestions(i, &host) {
                    Ok(list) => {
                        for s in list {
                            any = true;
                            println!("{i}: {}  [{}]\n    {}", s.title, s.key, s.detail);
                        }
                    }
                    Err(e) => println!("{i}: could not check: {e}"),
                }
            }
            if !any {
                println!("No suggestions.");
            } else {
                println!("\nApply one with: appofholding suggest <id> --apply <key>");
            }
        }
        Cmd::List => {
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
        Cmd::Remove {
            id,
            keep_file,
            delete_file,
            purge_data,
        } => {
            let lib = library()?;
            let policy = if delete_file {
                FilePolicy::Delete
            } else if keep_file {
                FilePolicy::Keep
            } else {
                FilePolicy::Auto
            };
            let rec = lib.remove_with(&id, policy)?;
            println!("Removed {} ({})", rec.name, rec.id);
            if rec.appimage_path.exists() {
                println!(
                    "The AppImage file was kept: {}",
                    rec.appimage_path.display()
                );
            }
            if purge_data {
                if lib.purge_private_home(&id)? {
                    println!("Deleted its private data folder.");
                }
            } else if lib.dirs.private_home(&id).is_dir() {
                println!(
                    "Its private data folder was kept: {}\n(add --purge-data to delete it)",
                    lib.dirs.private_home(&id).display()
                );
            }
        }
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
        Cmd::Apply {
            file,
            no_install,
            prune,
            dry_run,
            allow_commands,
        } => {
            let text = std::fs::read_to_string(&file)
                .with_context(|| format!("cannot read {}", file.display()))?;
            let cfg = Config::parse(&text)?;
            let lib = library()?;
            if dry_run {
                for a in &cfg.apps {
                    let what = if lib.get(&a.id).is_ok() {
                        "configure"
                    } else if a.file.is_some() {
                        "add from file, then configure"
                    } else if a.source.is_some() && !no_install {
                        "download, then configure"
                    } else {
                        "SKIP (not installed, nothing says how to get it)"
                    };
                    println!("{:<24} {what}", a.id);
                    for c in appofholding_core::config::commands_in(a) {
                        println!(
                            "{:<24}   runs a command: {c}{}",
                            "",
                            if allow_commands {
                                ""
                            } else {
                                "  (refused without --allow-commands)"
                            }
                        );
                    }
                }
                return Ok(());
            }
            let mut failed = false;
            for n in lib.apply_config(
                &UreqHttp::new(),
                &cfg,
                &ConfigApply {
                    no_install,
                    prune,
                    allow_commands,
                },
            ) {
                match n {
                    Note::Installed(id) => println!("{id}: downloaded and installed"),
                    Note::Added(id) => println!("{id}: added from file"),
                    Note::Configured(id) => println!("{id}: configured"),
                    Note::Missing { id, reason } => println!("{id}: skipped ({reason})"),
                    Note::Failed { id, error } => {
                        failed = true;
                        println!("{id}: FAILED: {error}");
                    }
                }
            }
            print_warnings(&lib);
            if failed {
                bail!("some entries could not be applied");
            }
        }
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
        Cmd::Check { id } => {
            let lib = library()?;
            let http = UreqHttp::new();
            let ids: Vec<String> = match id {
                Some(i) => vec![i],
                None => lib.sources()?.into_iter().map(|s| s.app_id).collect(),
            };
            if ids.is_empty() {
                println!(
                    "No apps have an update source yet. Use `appofholding source set <id> <link>`."
                );
            }
            for (i, result) in lib.check_many(&http, &ids) {
                match result {
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
                // The program itself: tell the user once when a new release is out.
                match selfupdate::notify_if_new(
                    &http,
                    &NotifySend,
                    &lib.dirs,
                    selfupdate::detect_current(),
                    Arch::host(),
                ) {
                    Ok(Some(msg)) => println!("appofholding: {msg}"),
                    Ok(None) => {}
                    Err(e) => println!("appofholding: could not check for a new release: {e}"),
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
                    "No apps have an update source yet. Use `appofholding source set <id> <link>`."
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
            print_warnings(&lib);
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
        Cmd::Launch(cmd) => launch_cmd(cmd)?,
        Cmd::Run {
            profile,
            print,
            unsandboxed,
            id,
            args,
        } => {
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
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_answers() {
        assert_eq!(parse_answer("y\n", 5), Answer::Selected);
        assert_eq!(parse_answer(" YES ", 5), Answer::Selected);
        for no in ["", "n", "no", "maybe", "0", "9", "1,x", "-1"] {
            assert_eq!(parse_answer(no, 5), Answer::No, "{no:?}");
        }
        assert_eq!(parse_answer("1,3", 5), Answer::Numbers(vec![1, 3]));
        assert_eq!(parse_answer("2 4", 5), Answer::Numbers(vec![2, 4]));
        assert_eq!(parse_answer("5", 5), Answer::Numbers(vec![5]));
    }

    #[test]
    fn paths_are_shortened_relative_to_home() {
        assert_eq!(
            short(
                Path::new("/home/u/AppImages/a.AppImage"),
                Path::new("/home/u")
            ),
            "~/AppImages/a.AppImage"
        );
        assert_eq!(
            short(Path::new("/opt/a.AppImage"), Path::new("/home/u")),
            "/opt/a.AppImage"
        );
    }
}
