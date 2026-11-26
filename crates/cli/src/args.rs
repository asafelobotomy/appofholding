//! The command line: every subcommand and its options.

use super::*;

/// Manage AppImages: integrate them into the desktop, track their update sources, and launch them.
#[derive(Parser)]
#[command(name = "appofholding", version, about)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Cmd,
}

#[derive(Subcommand)]
pub(crate) enum Cmd {
    /// Add an AppImage to the library and integrate it into the desktop menu.
    /// TARGET is a file, or a link to download from (GitHub/GitLab/Forgejo repo or release URL,
    /// `owner/repo`, or a direct URL).
    Add {
        target: String,
        /// Pick the release file with this pattern (`*` and `?` wildcards), e.g. 'Foo-*-x86_64.AppImage'.
        /// Only when adding from a link.
        #[arg(long)]
        asset: Option<String>,
        /// Which release to follow: latest, latest-pre, latest-all or a tag name. Only when
        /// adding from a link.
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
        /// Machine-readable output: one object at the end, nothing else on stdout.
        #[arg(long)]
        json: bool,
    },
    /// Have oysterAV scan the AppImages in the library, and keep what it says.
    Scan(scancmd::ScanArgs),
    /// List apps in the library.
    List {
        /// Print JSON (with each app's update policy and last check) instead of a table.
        #[arg(long)]
        json: bool,
    },
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
        /// Only show what would be removed.
        #[arg(long)]
        dry_run: bool,
        /// Machine-readable output: the plan with --dry-run, otherwise what was done.
        #[arg(long)]
        json: bool,
    },
    /// Inspect or change where an app gets its updates from.
    #[command(subcommand)]
    Source(SourceCmd),
    /// Check apps for updates (all apps with a source, or just ID).
    Check {
        id: Option<String>,
        /// The app whose menu entry has this desktop id (appofholding-<id>.desktop), instead of ID.
        #[arg(long, value_name = "DESKTOP_ID", conflicts_with = "id")]
        desktop_id: Option<String>,
        /// Print JSON instead of a table.
        #[arg(long)]
        json: bool,
        /// Also show the result as a desktop notification.
        #[arg(long)]
        notify: bool,
        /// One JSON line per step as it happens (start, one per app, done), for progress bars.
        #[arg(long, conflicts_with = "json")]
        ndjson: bool,
    },
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
        /// How many previous versions to keep for rollback (default: the library's setting,
        /// see `appofholding defaults`).
        #[arg(long)]
        keep: Option<usize>,
        /// Update ID from this AppImage instead of its source (a version you already
        /// downloaded). The file is moved into place unless --copy is given.
        #[arg(
            long,
            value_name = "FILE",
            requires = "id",
            conflicts_with = "scheduled"
        )]
        from: Option<PathBuf>,
        /// With --from: leave the file where it is and install a copy.
        #[arg(long, requires = "from")]
        copy: bool,
        /// Only show which updates would be installed (what the last check found).
        #[arg(long, conflicts_with_all = ["scheduled", "from", "ndjson"])]
        dry_run: bool,
        /// With --dry-run: print the plan as JSON.
        #[arg(long, requires = "dry_run")]
        json: bool,
        /// One JSON line per step as it happens (an app starts, its download percent, its
        /// result, done), for progress bars.
        #[arg(long, conflicts_with_all = ["scheduled", "from"])]
        ndjson: bool,
    },
    /// List the previous versions kept for rollback.
    Versions {
        id: String,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
    /// Go back to a kept version (the newest one unless --to is given).
    Rollback {
        id: String,
        /// Version string or SHA-256 prefix, as shown by `versions`.
        #[arg(long)]
        to: Option<String>,
        /// Only show what going back would change.
        #[arg(long)]
        dry_run: bool,
        /// Machine-readable output: the plan with --dry-run, otherwise what was done.
        #[arg(long)]
        json: bool,
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
    /// Show or change the settings that apply to every app.
    #[command(subcommand)]
    Defaults(DefaultsCmd),
    /// Show or change every setting in one place: the defaults above plus the background check
    /// schedule. The same list uQ Settings and the window show.
    #[command(subcommand)]
    Config(ConfigCmd),
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
pub(crate) enum SandboxCmd {
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
pub(crate) struct ImportArgs {
    #[command(subcommand)]
    pub(crate) action: Option<ImportAction>,
    /// Also look in this folder (repeatable).
    #[arg(long = "dir")]
    pub(crate) dirs: Vec<PathBuf>,
    /// Look only in the --dir folders, not the usual places.
    #[arg(long)]
    pub(crate) only_dirs: bool,
    /// How to bring in loose and tool-integrated files: in-place (leave them), copy, or move
    /// them into the library folder. Default: leave them, except files in Downloads/Desktop
    /// which are moved. AppImages an app manages itself always stay where they are.
    #[arg(long)]
    pub(crate) mode: Option<String>,
    /// Back up the menu entries other tools (Gear Lever, AppImageLauncher...) made for these
    /// AppImages, so each app appears once. Undo with `appofholding import restore`.
    #[arg(long)]
    pub(crate) replace_entries: bool,
    /// Import only these (names, ids or the numbers shown in the plan), comma separated.
    #[arg(long, value_delimiter = ',')]
    pub(crate) only: Vec<String>,
    /// Leave these out (names, ids or numbers).
    #[arg(long, value_delimiter = ',')]
    pub(crate) exclude: Vec<String>,
    /// Also import the ones needing a decision, under the suggested different id.
    #[arg(long)]
    pub(crate) include_conflicts: bool,
    /// Show the plan and stop; change nothing.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Do not ask for confirmation.
    #[arg(short, long)]
    pub(crate) yes: bool,
}

#[derive(Subcommand)]
pub(crate) enum ImportAction {
    /// Put back menu entries that `import --replace-entries` backed up.
    Restore {
        /// Which backup (see --list); default: the newest.
        batch: Option<String>,
        /// Only list the backups.
        #[arg(long)]
        list: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum SelfCmd {
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

#[derive(Subcommand)]
pub(crate) enum LaunchCmd {
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
pub(crate) enum HookCmd {
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
pub(crate) enum ScheduleCmd {
    /// Enable background update checks.
    Install {
        /// hourly, daily or weekly
        #[arg(long, default_value = "daily")]
        interval: String,
    },
    /// Disable background update checks.
    Remove,
    /// Show whether the timer is installed and when it runs next.
    Status {
        /// Print JSON ({"installed", "interval"}) instead.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum DefaultsCmd {
    /// Show the current defaults.
    Show {
        /// Print JSON ({"policy", "keepVersions", "sandbox"}) instead.
        #[arg(long)]
        json: bool,
    },
    /// Change one or more defaults.
    Set {
        /// The policy new update sources start with: auto, notify, manual or pinned.
        #[arg(long)]
        policy: Option<String>,
        /// Also give every app that has an update source this policy.
        #[arg(long, requires = "policy")]
        all: bool,
        /// How many previous versions an update keeps (0-10). Lowering it deletes the extra
        /// ones now.
        #[arg(long)]
        keep_versions: Option<usize>,
        /// The sandbox newly added apps start in: none, standard or strict.
        #[arg(long)]
        sandbox: Option<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum SourceCmd {
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
