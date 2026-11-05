//! Sandboxing with bubblewrap.
//!
//! A sandboxed app runs from its unpacked tree (see [`crate::extract`]) inside a fresh set of
//! namespaces with an empty root, an empty environment, and only the pieces its settings allow:
//! network, home access, GPU, audio, display sockets, the session bus (optionally filtered),
//! cameras, and extra folders the user granted. Building the `bwrap` command is a pure
//! function of the settings and a description of the host, so it is fully testable.
//!
//! This is real isolation, but weaker than Flatpak: there is no per-app D-Bus identity or
//! portal document store, and a Chromium/Electron app has to turn off its own (nested) sandbox.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// No sandbox: the app runs like any other program.
    #[default]
    None,
    /// Network, GPU, audio, display and a filtered session bus; private home folder.
    Standard,
    /// No network, no audio, no bus, no devices; private home folder; display and GPU only.
    Strict,
    /// Anything else (settings changed from a preset).
    Custom,
}

impl Level {
    pub fn parse(s: &str) -> Option<Level> {
        Some(match s {
            "none" | "off" => Level::None,
            "standard" => Level::Standard,
            "strict" => Level::Strict,
            "custom" => Level::Custom,
            _ => return None,
        })
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Level::None => "none",
            Level::Standard => "standard",
            Level::Strict => "strict",
            Level::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HomeAccess {
    /// The app gets its own persistent, empty home folder.
    Private,
    /// Your real home folder, read-only; the app's config/data/cache go to a private folder.
    ReadOnly,
    /// Your real home folder, writable. (Everything else stays isolated.)
    ReadWrite,
}

impl HomeAccess {
    pub fn parse(s: &str) -> Option<HomeAccess> {
        Some(match s {
            "private" | "none" => HomeAccess::Private,
            "ro" | "readonly" | "read-only" => HomeAccess::ReadOnly,
            "rw" | "readwrite" | "read-write" => HomeAccess::ReadWrite,
            _ => return None,
        })
    }
    pub fn as_str(self) -> &'static str {
        match self {
            HomeAccess::Private => "private",
            HomeAccess::ReadOnly => "ro",
            HomeAccess::ReadWrite => "rw",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Bus {
    Off,
    /// Through `xdg-dbus-proxy`: portals, notifications and tray icons only.
    Filtered,
    /// Your whole session bus (the app can talk to anything on it).
    Full,
}

impl Bus {
    pub fn parse(s: &str) -> Option<Bus> {
        Some(match s {
            "off" | "none" => Bus::Off,
            "filtered" | "filter" => Bus::Filtered,
            "full" | "on" => Bus::Full,
            _ => return None,
        })
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Bus::Off => "off",
            Bus::Filtered => "filtered",
            Bus::Full => "full",
        }
    }
}

/// A host folder made visible inside the sandbox at the same path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathGrant {
    pub path: PathBuf,
    pub write: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub level: Level,
    pub network: bool,
    pub home: HomeAccess,
    pub gpu: bool,
    pub audio: bool,
    /// Cameras and other media devices (`/dev/video*`, `/dev/media*`).
    pub devices: bool,
    /// Wayland / X11 sockets. Off = the app cannot open windows.
    pub display: bool,
    pub bus: Bus,
    pub paths: Vec<PathGrant>,
    /// Add `--no-sandbox` for Chromium/Electron apps, whose own sandbox cannot nest inside ours.
    pub chromium_workaround: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings::for_level(Level::None)
    }
}

impl Settings {
    pub fn for_level(level: Level) -> Settings {
        let base = Settings {
            level,
            network: true,
            home: HomeAccess::Private,
            gpu: true,
            audio: true,
            devices: false,
            display: true,
            bus: Bus::Filtered,
            paths: vec![],
            chromium_workaround: true,
        };
        match level {
            Level::None | Level::Standard | Level::Custom => base,
            Level::Strict => Settings {
                network: false,
                audio: false,
                bus: Bus::Off,
                ..base
            },
        }
    }

    pub fn enabled(&self) -> bool {
        self.level != Level::None
    }

    /// Mark as `custom` if the settings no longer match their preset.
    pub fn normalized(mut self) -> Settings {
        if matches!(self.level, Level::Standard | Level::Strict)
            && self != Settings::for_level(self.level)
        {
            self.level = Level::Custom;
        }
        self
    }

    pub fn validate(&self) -> Result<()> {
        for g in &self.paths {
            if !g.path.is_absolute() {
                return Err(Error::Other(format!(
                    "{} is not an absolute path",
                    g.path.display()
                )));
            }
            let s = g.path.to_string_lossy();
            if s == "/"
                || ["/proc", "/sys", "/dev", "/run"]
                    .iter()
                    .any(|p| s == *p || s.starts_with(&format!("{p}/")))
            {
                return Err(Error::Other(format!(
                    "{} cannot be shared with a sandbox",
                    g.path.display()
                )));
            }
            if s.contains('\0') {
                return Err(Error::Other("paths cannot contain NUL bytes".into()));
            }
        }
        Ok(())
    }
}

/// What the sandbox builder needs to know about the machine.
pub struct Host {
    pub uid: u32,
    pub home: PathBuf,
    pub runtime_dir: PathBuf,
    /// Selected environment variables (locale, desktop, user name, ...).
    pub env: BTreeMap<String, String>,
    pub wayland_display: Option<String>,
    pub x_display: Option<String>,
    pub xauthority: Option<PathBuf>,
    pub dbus_address: Option<String>,
    pub exists: Box<dyn Fn(&Path) -> bool>,
    /// Entries of `dir` whose names start with `prefix` (e.g. `/dev`, `nvidia`).
    pub list: Box<ListFn>,
}

/// Lists the entries of a directory whose names start with a prefix.
pub type ListFn = dyn Fn(&Path, &str) -> Vec<PathBuf>;

/// Variables copied from the launching environment when present.
const PASS_ENV: &[&str] = &[
    "USER",
    "LOGNAME",
    "LANG",
    "LANGUAGE",
    "LC_ALL",
    "LC_CTYPE",
    "LC_MESSAGES",
    "LC_TIME",
    "TZ",
    "TERM",
    "COLORTERM",
    "XDG_CURRENT_DESKTOP",
    "DESKTOP_SESSION",
    "XCURSOR_THEME",
    "XCURSOR_SIZE",
];

impl Host {
    pub fn detect() -> Result<Host> {
        use std::os::unix::fs::MetadataExt;
        let uid = std::fs::metadata("/proc/self")
            .map_err(Error::io("/proc/self"))?
            .uid();
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let home = var("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| Error::Other("$HOME is not set".into()))?;
        let runtime_dir = var("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(format!("/run/user/{uid}")));
        let env = PASS_ENV
            .iter()
            .filter_map(|k| var(k).map(|v| (k.to_string(), v)))
            .collect();
        Ok(Host {
            uid,
            home,
            runtime_dir,
            env,
            wayland_display: var("WAYLAND_DISPLAY"),
            x_display: var("DISPLAY"),
            xauthority: var("XAUTHORITY").map(PathBuf::from),
            dbus_address: var("DBUS_SESSION_BUS_ADDRESS"),
            exists: Box::new(|p| p.exists()),
            list: Box::new(|dir, prefix| {
                std::fs::read_dir(dir)
                    .map(|rd| {
                        let mut v: Vec<PathBuf> = rd
                            .flatten()
                            .filter(|e| e.file_name().to_string_lossy().starts_with(prefix))
                            .map(|e| e.path())
                            .collect();
                        v.sort();
                        v
                    })
                    .unwrap_or_default()
            }),
        })
    }
}

/// A helper process that must be running (and have created `ready`) before the sandbox starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Helper {
    pub argv: Vec<String>,
    pub ready: PathBuf,
}

#[derive(Debug)]
pub struct Sandboxed {
    /// `bwrap ... -- <command>`; the caller puts any launch prefix in front.
    pub argv: Vec<String>,
    pub helper: Option<Helper>,
    /// Things the user should know (applied workarounds, missing sockets, ...).
    pub notes: Vec<String>,
}

pub struct Launch<'a> {
    pub id: &'a str,
    /// The unpacked AppImage.
    pub appdir: &'a Path,
    /// The original AppImage (exposed read-only as `$APPIMAGE`).
    pub appimage: &'a Path,
    pub private_home: &'a Path,
    /// Where the filtered-bus socket is created (only used for `Bus::Filtered`).
    pub proxy_socket: &'a Path,
    pub app_args: &'a [String],
    pub env: &'a BTreeMap<String, String>,
    pub cwd: Option<&'a Path>,
}

/// Does this unpacked tree look like a Chromium/Electron app?
pub fn looks_like_chromium(appdir: &Path) -> bool {
    const MARKERS: &[&str] = &[
        "chrome-sandbox",
        "chrome_100_percent.pak",
        "libffmpeg.so",
        "v8_context_snapshot.bin",
    ];
    let has = |d: &Path| MARKERS.iter().any(|m| d.join(m).exists());
    if has(appdir) {
        return true;
    }
    std::fs::read_dir(appdir.join("opt"))
        .map(|rd| rd.flatten().any(|e| has(&e.path())))
        .unwrap_or(false)
}

/// `--ro-bind /usr` plus `/bin`, `/sbin`, `/lib`, `/lib64`: on merged-/usr systems these are
/// links into `/usr`, recreated with their real target (Arch: `lib64 -> usr/lib`; Fedora, Debian
/// and Ubuntu: `lib64 -> usr/lib64`); elsewhere they are bound read-only.
pub fn base_system_args(exists: &dyn Fn(&Path) -> bool) -> Vec<String> {
    let mut a: Vec<String> = ["--ro-bind", "/usr", "/usr"].map(String::from).to_vec();
    for d in ["bin", "sbin", "lib", "lib64"] {
        let p = PathBuf::from("/").join(d);
        if !exists(&p) {
            continue;
        }
        match std::fs::read_link(&p) {
            Ok(target) => a.extend([
                "--symlink".into(),
                target.to_string_lossy().into_owned(),
                format!("/{d}"),
            ]),
            Err(_) => a.extend(["--ro-bind".into(), format!("/{d}"), format!("/{d}")]),
        }
    }
    a
}

/// The smallest sandbox that runs `/bin/true`: answers "can bubblewrap work here at all?".
pub fn probe_args() -> Vec<String> {
    let mut a = vec!["--unshare-all".to_string()];
    a.extend(base_system_args(&|p: &Path| p.exists()));
    a.extend(["--proc", "/proc", "--dev", "/dev", "/bin/true"].map(String::from));
    a
}

/// Bus name prefixes that belong to the desktop and its services. An app may not claim names
/// under them, whatever its own `.desktop` files say.
const RESERVED_BUS_PREFIXES: &[&str] = &[
    "org.freedesktop.",
    "org.gnome.",
    "org.gtk.",
    "org.kde.kwalletd",
    "org.kde.KWin",
    "org.kde.plasmashell",
    "org.kde.StatusNotifier",
    "org.kde.kglobalaccel",
    "org.kde.ksmserver",
    "org.a11y.",
    "com.canonical.",
    "ca.desrt.",
];

/// A name the app may own: a valid bus name with at least three parts (so `org.kde` or
/// `com.example` can never become a wildcard over a whole vendor) outside the desktop's names.
fn ownable_bus_name(n: &str) -> bool {
    valid_bus_name(n)
        && n.split('.').count() >= 3
        && !RESERVED_BUS_PREFIXES
            .iter()
            .any(|p| n.starts_with(p) || format!("{n}.").starts_with(p))
}

fn valid_bus_name(n: &str) -> bool {
    let parts: Vec<&str> = n.split('.').collect();
    parts.len() >= 2
        && parts.len() <= 8
        && n.len() <= 255
        && parts.iter().all(|p| {
            !p.is_empty()
                && !p.starts_with(|c: char| c.is_ascii_digit())
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
}

/// D-Bus names an app may need to own: reverse-DNS names found in its top-level `.desktop`
/// files (file names and `StartupWMClass`).
pub fn dbus_names(appdir: &Path) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let Ok(rd) = std::fs::read_dir(appdir) else {
        return names;
    };
    let mut files: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "desktop"))
        .collect();
    files.sort();
    for f in files {
        let mut cands: Vec<String> = f
            .file_stem()
            .map(|s| vec![s.to_string_lossy().into_owned()])
            .unwrap_or_default();
        if let Ok(text) = std::fs::read_to_string(&f) {
            cands.extend(
                text.lines()
                    .filter_map(|l| l.strip_prefix("StartupWMClass="))
                    .map(|s| s.trim().to_string()),
            );
        }
        for c in cands {
            if ownable_bus_name(&c) && !names.contains(&c) && names.len() < 8 {
                names.push(c);
            }
        }
    }
    names
}

fn push(v: &mut Vec<String>, items: &[&str]) {
    v.extend(items.iter().map(|s| s.to_string()));
}

fn x11_socket(display: &str) -> Option<PathBuf> {
    // ":1", ":1.0" and "unix:1" are local displays; "host:N" is a network display.
    let rest = display
        .strip_prefix("unix:")
        .or_else(|| display.strip_prefix(':'))?;
    let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    (!num.is_empty()).then(|| PathBuf::from(format!("/tmp/.X11-unix/X{num}")))
}

fn dbus_path(addr: &str) -> Option<PathBuf> {
    addr.split(';').find_map(|a| {
        a.strip_prefix("unix:")?
            .split(',')
            .find_map(|kv| kv.strip_prefix("path=").map(PathBuf::from))
    })
}

/// Files and folders under `/etc` that programs need to start, resolve names, and find fonts and
/// certificates. Everything else in `/etc` stays out.
const ETC: &[&str] = &[
    "ld.so.cache",
    "ld.so.conf",
    "ld.so.conf.d",
    "ssl",
    "ca-certificates",
    "pki",
    "fonts",
    "localtime",
    "os-release",
    "passwd",
    "group",
    "nsswitch.conf",
    "hosts",
    "host.conf",
    "alternatives",
    "xdg",
    "gtk-3.0",
    "vulkan",
    "glvnd",
    "drirc",
    "mime.types",
    "machine-id",
    "timezone",
    "login.defs",
    "protocols",
    "services",
];

/// Build the `bwrap` command line for `launch` under `settings`.
pub fn build(settings: &Settings, host: &Host, launch: &Launch<'_>) -> Result<Sandboxed> {
    settings.validate()?;
    if !settings.enabled() {
        return Err(Error::Other("sandboxing is off for this app".into()));
    }
    let ex = |p: &Path| (host.exists)(p);
    let mut notes: Vec<String> = Vec::new();
    let mut a: Vec<String> = vec!["bwrap".into()];
    push(
        &mut a,
        &[
            "--die-with-parent",
            "--new-session",
            "--unshare-pid",
            "--unshare-ipc",
            "--unshare-uts",
            "--unshare-cgroup-try",
            "--cap-drop",
            "ALL",
            "--clearenv",
        ],
    );
    if !settings.network {
        a.push("--unshare-net".into());
    }

    // --- base system, read-only
    a.extend(base_system_args(&ex));
    for e in ETC {
        let p = PathBuf::from("/etc").join(e);
        if ex(&p) {
            a.extend([
                "--ro-bind".into(),
                p.display().to_string(),
                p.display().to_string(),
            ]);
        }
    }
    if settings.network {
        // /etc/resolv.conf is usually a link into here.
        for p in ["/etc/resolv.conf", "/run/systemd/resolve"] {
            if ex(Path::new(p)) {
                a.extend(["--ro-bind".into(), p.into(), p.into()]);
            }
        }
    }
    push(
        &mut a,
        &[
            "--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp", "--tmpfs", "/var/tmp",
        ],
    );
    let rt = host.runtime_dir.display().to_string();
    a.extend(["--perms".into(), "0700".into(), "--dir".into(), rt.clone()]);

    // --- the app itself
    let image_name = launch
        .appimage
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "app.AppImage".into());
    let image_in = format!("/run/appofholding/{image_name}");
    a.extend([
        "--ro-bind".into(),
        launch.appdir.display().to_string(),
        "/app".into(),
    ]);
    a.extend([
        "--ro-bind".into(),
        launch.appimage.display().to_string(),
        image_in.clone(),
    ]);

    // --- home
    let home = host.home.display().to_string();
    let mut env: BTreeMap<String, String> = BTreeMap::new();
    env.insert("HOME".into(), home.clone());
    match settings.home {
        HomeAccess::Private => {
            a.extend([
                "--bind".into(),
                launch.private_home.display().to_string(),
                home.clone(),
            ]);
        }
        HomeAccess::ReadWrite => a.extend(["--bind".into(), home.clone(), home.clone()]),
        HomeAccess::ReadOnly => {
            a.extend(["--ro-bind".into(), home.clone(), home.clone()]);
            // The app still needs somewhere to write its own state.
            let p = "/run/appofholding/home";
            a.extend([
                "--bind".into(),
                launch.private_home.display().to_string(),
                p.into(),
            ]);
            env.insert("XDG_CONFIG_HOME".into(), format!("{p}/.config"));
            env.insert("XDG_DATA_HOME".into(), format!("{p}/.local/share"));
            env.insert("XDG_STATE_HOME".into(), format!("{p}/.local/state"));
            env.insert("XDG_CACHE_HOME".into(), format!("{p}/.cache"));
        }
    }
    if settings.home == HomeAccess::Private {
        // Fonts, themes and icons so the app looks like the rest of the desktop.
        for rel in [
            ".local/share/fonts",
            ".config/fontconfig",
            ".themes",
            ".icons",
            ".local/share/icons",
        ] {
            let p = host.home.join(rel);
            if ex(&p) {
                a.extend([
                    "--ro-bind".into(),
                    p.display().to_string(),
                    p.display().to_string(),
                ]);
            }
        }
    }
    for g in &settings.paths {
        if !ex(&g.path) {
            return Err(Error::Other(format!(
                "{} does not exist, so it cannot be shared",
                g.path.display()
            )));
        }
        let flag = if g.write { "--bind" } else { "--ro-bind" };
        a.extend([
            flag.into(),
            g.path.display().to_string(),
            g.path.display().to_string(),
        ]);
    }

    // --- GPU
    if settings.gpu {
        for p in ["/sys/dev/char", "/sys/devices", "/sys/class"] {
            if ex(Path::new(p)) {
                a.extend(["--ro-bind".into(), p.into(), p.into()]);
            }
        }
        let dri = Path::new("/dev/dri");
        if ex(dri) {
            a.extend(["--dev-bind".into(), "/dev/dri".into(), "/dev/dri".into()]);
        }
        for p in (host.list)(Path::new("/dev"), "nvidia") {
            a.extend([
                "--dev-bind".into(),
                p.display().to_string(),
                p.display().to_string(),
            ]);
        }
    }
    if settings.devices {
        for prefix in ["video", "media"] {
            for p in (host.list)(Path::new("/dev"), prefix) {
                a.extend([
                    "--dev-bind".into(),
                    p.display().to_string(),
                    p.display().to_string(),
                ]);
            }
        }
    }

    // --- display
    if settings.display {
        let mut any = false;
        if let Some(w) = &host.wayland_display {
            let sock = host.runtime_dir.join(w);
            if ex(&sock) {
                a.extend([
                    "--bind".into(),
                    sock.display().to_string(),
                    sock.display().to_string(),
                ]);
                env.insert("WAYLAND_DISPLAY".into(), w.clone());
                any = true;
            }
        }
        if let Some(d) = &host.x_display
            && let Some(sock) = x11_socket(d)
            && ex(&sock)
        {
            a.extend([
                "--bind".into(),
                sock.display().to_string(),
                sock.display().to_string(),
            ]);
            env.insert("DISPLAY".into(), d.clone());
            any = true;
            if let Some(xa) = host.xauthority.as_ref().filter(|p| ex(p)) {
                a.extend([
                    "--ro-bind".into(),
                    xa.display().to_string(),
                    xa.display().to_string(),
                ]);
                env.insert("XAUTHORITY".into(), xa.display().to_string());
            }
        }
        if !any {
            notes.push(
                "no Wayland or X11 socket found; the app may not be able to open a window".into(),
            );
        }
    }

    // --- audio
    if settings.audio {
        let mut any = false;
        let pw = host.runtime_dir.join("pipewire-0");
        if ex(&pw) {
            a.extend([
                "--bind".into(),
                pw.display().to_string(),
                pw.display().to_string(),
            ]);
            any = true;
        }
        let pulse = host.runtime_dir.join("pulse/native");
        if ex(&pulse) {
            a.extend([
                "--bind".into(),
                pulse.display().to_string(),
                pulse.display().to_string(),
            ]);
            env.insert("PULSE_SERVER".into(), format!("unix:{}", pulse.display()));
            let cookie = host.home.join(".config/pulse/cookie");
            if ex(&cookie) {
                a.extend([
                    "--ro-bind".into(),
                    cookie.display().to_string(),
                    cookie.display().to_string(),
                ]);
            }
            any = true;
        }
        if !any {
            notes.push("no PipeWire or PulseAudio socket found; the app will have no sound".into());
        }
    }

    // --- session bus
    let mut helper = None;
    let bus_in = format!("{rt}/bus");
    match settings.bus {
        Bus::Off => {}
        Bus::Full => {
            let path = host
                .dbus_address
                .as_deref()
                .and_then(dbus_path)
                .filter(|p| ex(p));
            match path {
                Some(p) => {
                    a.extend(["--bind".into(), p.display().to_string(), bus_in.clone()]);
                    env.insert(
                        "DBUS_SESSION_BUS_ADDRESS".into(),
                        format!("unix:path={bus_in}"),
                    );
                }
                None => notes.push("no session bus socket found; D-Bus is unavailable".into()),
            }
        }
        Bus::Filtered => {
            let addr = host.dbus_address.as_deref().ok_or_else(|| {
                Error::Other(
                    "no session D-Bus address in the environment; use `--bus off` or `--bus full`"
                        .into(),
                )
            })?;
            let sock = launch.proxy_socket.display().to_string();
            helper = Some(Helper {
                argv: vec![
                    "xdg-dbus-proxy".into(),
                    addr.to_string(),
                    sock.clone(),
                    // stdin is a pipe whose write end we hold: the proxy exits when we do,
                    // even if we are killed without a chance to clean up.
                    "--fd=0".into(),
                    "--filter".into(),
                    "--talk=org.freedesktop.portal.*".into(),
                    "--talk=org.freedesktop.Notifications".into(),
                    "--talk=org.kde.StatusNotifierWatcher".into(),
                    "--talk=org.freedesktop.StatusNotifierWatcher".into(),
                    "--talk=org.freedesktop.ScreenSaver".into(),
                    "--talk=org.freedesktop.FileManager1".into(),
                    "--call=org.freedesktop.portal.*=*".into(),
                    "--broadcast=org.freedesktop.portal.*=@/org/freedesktop/portal/*".into(),
                ],
                ready: launch.proxy_socket.to_path_buf(),
            });
            // GTK and Qt apps register a bus name derived from their application id at start-up
            // and abort if they are not allowed to; allow exactly the names this app declares.
            if let Some(h) = helper.as_mut() {
                for n in dbus_names(launch.appdir) {
                    h.argv.push(format!("--own={n}"));
                    h.argv.push(format!("--own={n}.*"));
                }
            }
            a.extend(["--bind".into(), sock, bus_in.clone()]);
            env.insert(
                "DBUS_SESSION_BUS_ADDRESS".into(),
                format!("unix:path={bus_in}"),
            );
        }
    }

    // Tell toolkits which display protocol is really available (the host's own value may name
    // a socket that was not shared).
    if env.contains_key("WAYLAND_DISPLAY") {
        env.insert("XDG_SESSION_TYPE".into(), "wayland".into());
    } else if env.contains_key("DISPLAY") {
        env.insert("XDG_SESSION_TYPE".into(), "x11".into());
    }

    // --- environment (cleared above; everything here is explicit)
    env.insert("PATH".into(), "/usr/local/bin:/usr/bin:/bin".into());
    env.insert("XDG_RUNTIME_DIR".into(), rt.clone());
    env.insert("APPDIR".into(), "/app".into());
    env.insert("APPIMAGE".into(), image_in);
    // Lets the host find this instance (to defer updates, and to keep its unpacked tree).
    env.insert(
        crate::running::HOST_APPIMAGE_VAR.into(),
        launch.appimage.display().to_string(),
    );
    env.insert(
        crate::running::TREE_VAR.into(),
        launch.appdir.display().to_string(),
    );
    for (k, v) in &host.env {
        env.entry(k.clone()).or_insert_with(|| v.clone());
    }

    // --- command
    let mut app_args = launch.app_args.to_vec();
    if settings.chromium_workaround
        && looks_like_chromium(launch.appdir)
        && !app_args.iter().any(|x| x == "--no-sandbox")
    {
        app_args.push("--no-sandbox".into());
        notes.push("Chromium/Electron app: added --no-sandbox (its own sandbox cannot nest; this sandbox protects it instead)".into());
    }
    env.extend(launch.env.clone()); // the profile's own variables win
    let cwd = launch
        .cwd
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| home.clone());
    env.insert("OWD".into(), cwd.clone());
    for (k, v) in &env {
        a.extend(["--setenv".into(), k.clone(), v.clone()]);
    }
    a.extend(["--chdir".into(), cwd, "--".into(), "/app/AppRun".into()]);
    a.extend(app_args);
    Ok(Sandboxed {
        argv: a,
        helper,
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn host(present: &[&str]) -> Host {
        let set: BTreeSet<PathBuf> = present.iter().map(PathBuf::from).collect();
        let set2 = set.clone();
        Host {
            uid: 1000,
            home: "/home/u".into(),
            runtime_dir: "/run/user/1000".into(),
            env: [("LANG".to_string(), "en_US.UTF-8".to_string())].into(),
            wayland_display: Some("wayland-0".into()),
            x_display: Some(":1".into()),
            xauthority: Some("/run/user/1000/xauth_x".into()),
            dbus_address: Some("unix:path=/run/user/1000/bus".into()),
            exists: Box::new(move |p| set.contains(p)),
            list: Box::new(move |_, prefix| {
                set2.iter()
                    .filter(|p| {
                        p.file_name()
                            .is_some_and(|n| n.to_string_lossy().starts_with(prefix))
                    })
                    .cloned()
                    .collect()
            }),
        }
    }

    const FULL: &[&str] = &[
        "/bin",
        "/lib",
        "/etc/passwd",
        "/etc/ssl",
        "/etc/resolv.conf",
        "/run/systemd/resolve",
        "/dev/dri",
        "/dev/nvidia0",
        "/dev/video0",
        "/sys/devices",
        "/run/user/1000/wayland-0",
        "/tmp/.X11-unix/X1",
        "/run/user/1000/xauth_x",
        "/run/user/1000/pipewire-0",
        "/run/user/1000/pulse/native",
        "/run/user/1000/bus",
        "/home/u/.themes",
        "/home/u/Documents",
        "/home/u/Music",
    ];

    fn launch<'a>(env: &'a BTreeMap<String, String>, args: &'a [String]) -> Launch<'a> {
        Launch {
            id: "foo",
            appdir: Path::new("/cache/extracted/abc"),
            appimage: Path::new("/home/u/Applications/Foo.AppImage"),
            private_home: Path::new("/data/apps/foo/home"),
            proxy_socket: Path::new("/run/user/1000/appofholding-foo-bus"),
            app_args: args,
            env,
            cwd: None,
        }
    }

    fn has_seq(a: &[String], seq: &[&str]) -> bool {
        a.windows(seq.len())
            .any(|w| w.iter().zip(seq).all(|(x, y)| x == y))
    }

    fn setenv(a: &[String], k: &str) -> Option<String> {
        a.windows(3)
            .find(|w| w[0] == "--setenv" && w[1] == k)
            .map(|w| w[2].clone())
    }

    #[test]
    fn levels_and_normalisation() {
        assert!(!Settings::for_level(Level::None).enabled());
        let std = Settings::for_level(Level::Standard);
        assert!(
            std.network
                && std.audio
                && std.gpu
                && std.bus == Bus::Filtered
                && std.home == HomeAccess::Private
        );
        let strict = Settings::for_level(Level::Strict);
        assert!(!strict.network && !strict.audio && strict.bus == Bus::Off);
        // Tweaking a preset turns it into "custom"; an untouched preset stays itself.
        assert_eq!(std.clone().normalized().level, Level::Standard);
        assert_eq!(
            Settings {
                network: false,
                ..std
            }
            .normalized()
            .level,
            Level::Custom
        );
        assert_eq!(
            Settings {
                devices: true,
                ..strict
            }
            .normalized()
            .level,
            Level::Custom
        );
    }

    #[test]
    fn path_grants_are_validated() {
        let mk = |p: &str| Settings {
            paths: vec![PathGrant {
                path: p.into(),
                write: false,
            }],
            ..Settings::for_level(Level::Standard)
        };
        assert!(mk("/home/u/Music").validate().is_ok());
        for bad in [
            "relative/x",
            "/",
            "/proc",
            "/sys/devices",
            "/dev/dri",
            "/run/user/1000",
        ] {
            assert!(mk(bad).validate().is_err(), "{bad} should be refused");
        }
    }

    #[test]
    fn standard_profile_command() {
        let env = BTreeMap::new();
        let args = vec!["--flag".to_string(), "file".to_string()];
        let s = build(
            &Settings::for_level(Level::Standard),
            &host(FULL),
            &launch(&env, &args),
        )
        .unwrap();
        let a = &s.argv;
        assert_eq!(a[0], "bwrap");
        assert!(
            a.contains(&"--clearenv".to_string()) && a.contains(&"--die-with-parent".to_string())
        );
        assert!(
            !a.contains(&"--unshare-net".to_string()),
            "network stays on"
        );
        assert!(has_seq(a, &["--ro-bind", "/cache/extracted/abc", "/app"]));
        assert!(has_seq(
            a,
            &[
                "--ro-bind",
                "/home/u/Applications/Foo.AppImage",
                "/run/appofholding/Foo.AppImage"
            ]
        ));
        assert!(
            has_seq(a, &["--bind", "/data/apps/foo/home", "/home/u"]),
            "private home replaces the real one"
        );
        assert!(!has_seq(a, &["--bind", "/home/u", "/home/u"]));
        assert!(
            has_seq(a, &["--ro-bind", "/home/u/.themes", "/home/u/.themes"]),
            "theme folders are shared read-only"
        );
        assert!(
            !a.iter().any(|x| x.contains("Documents")),
            "other home folders are not exposed"
        );
        assert!(
            has_seq(a, &["--dev-bind", "/dev/dri", "/dev/dri"])
                && has_seq(a, &["--dev-bind", "/dev/nvidia0", "/dev/nvidia0"])
        );
        assert!(!a.iter().any(|x| x == "/dev/video0"), "cameras are off");
        assert!(has_seq(
            a,
            &[
                "--bind",
                "/run/user/1000/wayland-0",
                "/run/user/1000/wayland-0"
            ]
        ));
        assert!(has_seq(
            a,
            &["--bind", "/tmp/.X11-unix/X1", "/tmp/.X11-unix/X1"]
        ));
        assert!(has_seq(
            a,
            &[
                "--bind",
                "/run/user/1000/pipewire-0",
                "/run/user/1000/pipewire-0"
            ]
        ));
        assert_eq!(setenv(a, "WAYLAND_DISPLAY").as_deref(), Some("wayland-0"));
        assert_eq!(setenv(a, "APPDIR").as_deref(), Some("/app"));
        assert_eq!(setenv(a, "LANG").as_deref(), Some("en_US.UTF-8"));
        assert_eq!(setenv(a, "HOME").as_deref(), Some("/home/u"));
        // Ends with the command, after `--`.
        let dd = a.iter().position(|x| x == "--").unwrap();
        assert_eq!(&a[dd + 1..], ["/app/AppRun", "--flag", "file"]);
        // Filtered bus: a proxy helper and the proxy socket mounted as the bus.
        let h = s.helper.unwrap();
        assert_eq!(h.argv[0], "xdg-dbus-proxy");
        assert!(
            h.argv.contains(&"--filter".to_string())
                && h.argv
                    .iter()
                    .any(|x| x.starts_with("--talk=org.freedesktop.portal"))
        );
        assert!(has_seq(
            a,
            &[
                "--bind",
                "/run/user/1000/appofholding-foo-bus",
                "/run/user/1000/bus"
            ]
        ));
    }

    #[test]
    fn strict_profile_has_no_network_audio_or_bus() {
        let env = BTreeMap::new();
        let s = build(
            &Settings::for_level(Level::Strict),
            &host(FULL),
            &launch(&env, &[]),
        )
        .unwrap();
        let a = &s.argv;
        assert!(a.contains(&"--unshare-net".to_string()));
        assert!(
            !a.iter()
                .any(|x| x.contains("pipewire") || x.contains("pulse"))
        );
        assert!(!a.iter().any(|x| x.contains("resolv.conf")));
        assert!(s.helper.is_none() && setenv(a, "DBUS_SESSION_BUS_ADDRESS").is_none());
    }

    #[test]
    fn home_modes() {
        let env = BTreeMap::new();
        let mk = |h| Settings {
            home: h,
            ..Settings::for_level(Level::Standard)
        };
        let rw = build(&mk(HomeAccess::ReadWrite), &host(FULL), &launch(&env, &[]))
            .unwrap()
            .argv;
        assert!(has_seq(&rw, &["--bind", "/home/u", "/home/u"]));
        let ro = build(&mk(HomeAccess::ReadOnly), &host(FULL), &launch(&env, &[]))
            .unwrap()
            .argv;
        assert!(has_seq(&ro, &["--ro-bind", "/home/u", "/home/u"]));
        assert!(has_seq(
            &ro,
            &["--bind", "/data/apps/foo/home", "/run/appofholding/home"]
        ));
        assert_eq!(
            setenv(&ro, "XDG_CONFIG_HOME").as_deref(),
            Some("/run/appofholding/home/.config")
        );
    }

    #[test]
    fn granted_paths_devices_and_missing_paths() {
        let env = BTreeMap::new();
        let s = Settings {
            devices: true,
            paths: vec![
                PathGrant {
                    path: "/home/u/Music".into(),
                    write: false,
                },
                PathGrant {
                    path: "/home/u/Documents".into(),
                    write: true,
                },
            ],
            ..Settings::for_level(Level::Standard)
        };
        let a = build(&s, &host(FULL), &launch(&env, &[])).unwrap().argv;
        assert!(has_seq(
            &a,
            &["--ro-bind", "/home/u/Music", "/home/u/Music"]
        ));
        assert!(has_seq(
            &a,
            &["--bind", "/home/u/Documents", "/home/u/Documents"]
        ));
        assert!(has_seq(&a, &["--dev-bind", "/dev/video0", "/dev/video0"]));
        let missing = Settings {
            paths: vec![PathGrant {
                path: "/home/u/Nope".into(),
                write: false,
            }],
            ..Settings::for_level(Level::Standard)
        };
        assert!(
            build(&missing, &host(FULL), &launch(&env, &[]))
                .unwrap_err()
                .to_string()
                .contains("does not exist")
        );
    }

    #[test]
    fn profile_env_wins_and_other_host_env_is_dropped() {
        let env: BTreeMap<String, String> = [
            ("LANG".to_string(), "C".to_string()),
            ("MY".to_string(), "1".to_string()),
        ]
        .into();
        let a = build(
            &Settings::for_level(Level::Standard),
            &host(FULL),
            &launch(&env, &[]),
        )
        .unwrap()
        .argv;
        assert_eq!(setenv(&a, "LANG").as_deref(), Some("C"));
        assert_eq!(setenv(&a, "MY").as_deref(), Some("1"));
        assert!(setenv(&a, "SSH_AUTH_SOCK").is_none() && setenv(&a, "GITHUB_TOKEN").is_none());
    }

    #[test]
    fn bus_modes_and_missing_sockets_produce_notes_not_silent_failures() {
        let env = BTreeMap::new();
        let full = Settings {
            bus: Bus::Full,
            ..Settings::for_level(Level::Standard)
        };
        let a = build(&full, &host(FULL), &launch(&env, &[])).unwrap().argv;
        assert!(has_seq(
            &a,
            &["--bind", "/run/user/1000/bus", "/run/user/1000/bus"]
        ));
        // No display / audio / bus sockets at all (headless machine).
        let mut h = host(&["/bin", "/lib"]);
        h.dbus_address = None;
        let bare = Settings {
            bus: Bus::Full,
            ..Settings::for_level(Level::Standard)
        };
        let s = build(&bare, &h, &launch(&env, &[])).unwrap();
        let n = s.notes.join("|");
        assert!(
            n.contains("no Wayland or X11")
                && n.contains("no sound")
                && n.contains("D-Bus is unavailable"),
            "{n}"
        );
        // A filtered bus without an address is an error with a way out.
        let err = build(
            &Settings::for_level(Level::Standard),
            &h,
            &launch(&env, &[]),
        )
        .err()
        .unwrap()
        .to_string();
        assert!(err.contains("--bus off"), "{err}");
    }

    #[test]
    fn chromium_apps_get_no_sandbox_once() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("chrome-sandbox"), b"").unwrap();
        assert!(looks_like_chromium(tmp.path()));
        let env = BTreeMap::new();
        let mut l = launch(&env, &[]);
        l.appdir = tmp.path();
        let s = build(&Settings::for_level(Level::Standard), &host(FULL), &l).unwrap();
        assert_eq!(s.argv.last().map(String::as_str), Some("--no-sandbox"));
        assert!(s.notes.iter().any(|n| n.contains("--no-sandbox")));
        // Already present: not duplicated. Disabled: not added.
        let args = vec!["--no-sandbox".to_string()];
        let mut l2 = launch(&env, &args);
        l2.appdir = tmp.path();
        let s = build(&Settings::for_level(Level::Standard), &host(FULL), &l2).unwrap();
        assert_eq!(s.argv.iter().filter(|x| *x == "--no-sandbox").count(), 1);
        let off = Settings {
            chromium_workaround: false,
            ..Settings::for_level(Level::Standard)
        };
        assert!(
            !build(&off, &host(FULL), &l)
                .unwrap()
                .argv
                .contains(&"--no-sandbox".to_string())
        );
        // Nested layout used by some Electron AppImages.
        let nested = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(nested.path().join("opt/App")).unwrap();
        std::fs::write(nested.path().join("opt/App/libffmpeg.so"), b"").unwrap();
        assert!(looks_like_chromium(nested.path()));
        assert!(!looks_like_chromium(Path::new("/nonexistent")));
    }

    #[test]
    fn base_system_links_keep_their_real_target() {
        let args = base_system_args(&|p: &Path| p.exists());
        assert_eq!(&args[..3], ["--ro-bind", "/usr", "/usr"]);
        for d in ["bin", "sbin", "lib", "lib64"] {
            let p = PathBuf::from("/").join(d);
            if let Ok(t) = std::fs::read_link(&p) {
                let at = args.iter().position(|a| a == &format!("/{d}")).unwrap();
                assert_eq!(args[at - 2], "--symlink");
                assert_eq!(args[at - 1], t.to_string_lossy());
            }
        }
    }

    #[test]
    fn declared_application_ids_may_own_their_bus_name() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("emobie.desktop"),
            "[Desktop Entry]\nName=x\nStartupWMClass=io.github.me.emobie\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("io.github.me.emobie.desktop"),
            "[Desktop Entry]\nName=x\n",
        )
        .unwrap();
        std::fs::write(tmp.path().join("9bad.name.desktop"), "x").unwrap();
        let names = dbus_names(tmp.path());
        assert_eq!(
            names,
            vec!["io.github.me.emobie".to_string()],
            "no dot / invalid names are ignored: {names:?}"
        );
        assert!(dbus_names(Path::new("/nonexistent")).is_empty());
        assert!(
            valid_bus_name("org.kde.krita")
                && !valid_bus_name("nodots")
                && !valid_bus_name("a..b")
                && !valid_bus_name("org.1x")
        );
        // An AppImage cannot use its own file names to claim a vendor or the desktop's services.
        for bad in [
            "org.freedesktop",
            "org.kde",
            "org.freedesktop.secrets",
            "org.freedesktop.Notifications",
            "org.kde.kwalletd6",
            "org.gnome.keyring",
        ] {
            assert!(!ownable_bus_name(bad), "{bad} must not be ownable");
        }
        assert!(ownable_bus_name("org.kde.krita") && ownable_bus_name("io.github.me.emobie"));

        let env = BTreeMap::new();
        let mut l = launch(&env, &[]);
        l.appdir = tmp.path();
        let h = build(&Settings::for_level(Level::Standard), &host(FULL), &l)
            .unwrap()
            .helper
            .unwrap();
        assert!(
            h.argv.contains(&"--own=io.github.me.emobie".to_string())
                && h.argv.contains(&"--own=io.github.me.emobie.*".to_string())
        );
        assert!(
            !h.argv.iter().any(|a| a == "--own=*"),
            "never a blanket grant"
        );
    }

    #[test]
    fn session_type_matches_the_sockets_actually_shared() {
        let env = BTreeMap::new();
        let only_x = host(&["/bin", "/tmp/.X11-unix/X1"]);
        let a = build(
            &Settings {
                bus: Bus::Off,
                ..Settings::for_level(Level::Standard)
            },
            &only_x,
            &launch(&env, &[]),
        )
        .unwrap()
        .argv;
        assert_eq!(setenv(&a, "XDG_SESSION_TYPE").as_deref(), Some("x11"));
        let a = build(
            &Settings {
                bus: Bus::Off,
                ..Settings::for_level(Level::Standard)
            },
            &host(FULL),
            &launch(&env, &[]),
        )
        .unwrap()
        .argv;
        assert_eq!(setenv(&a, "XDG_SESSION_TYPE").as_deref(), Some("wayland"));
        let none = Settings {
            display: false,
            bus: Bus::Off,
            ..Settings::for_level(Level::Standard)
        };
        assert!(
            setenv(
                &build(&none, &host(FULL), &launch(&env, &[])).unwrap().argv,
                "XDG_SESSION_TYPE"
            )
            .is_none()
        );
    }

    #[test]
    fn display_parsing_helpers() {
        assert_eq!(x11_socket(":1"), Some("/tmp/.X11-unix/X1".into()));
        assert_eq!(x11_socket(":0.0"), Some("/tmp/.X11-unix/X0".into()));
        assert_eq!(x11_socket("unix:2"), Some("/tmp/.X11-unix/X2".into()));
        assert_eq!(x11_socket("remote:10"), None);
        assert_eq!(
            dbus_path("unix:path=/run/user/1000/bus,guid=abc"),
            Some("/run/user/1000/bus".into())
        );
        assert_eq!(dbus_path("unix:abstract=/tmp/dbus-x"), None);
    }

    // ---- Tests that really run bubblewrap. They are skipped (with a note) where it is unusable.

    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    fn bwrap_works() -> bool {
        Command::new("bwrap")
            .args(probe_args())
            .output()
            .is_ok_and(|o| o.status.success())
    }

    struct Rig {
        dir: tempfile::TempDir,
        appdir: PathBuf,
        home: PathBuf,
        private: PathBuf,
        image: PathBuf,
    }

    fn rig(apprun: &str) -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let appdir = dir.path().join("appdir");
        std::fs::create_dir(&appdir).unwrap();
        std::fs::write(appdir.join("AppRun"), apprun).unwrap();
        std::fs::set_permissions(
            appdir.join("AppRun"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        let home = dir.path().join("realhome");
        std::fs::create_dir(&home).unwrap();
        let private = dir.path().join("private");
        std::fs::create_dir(&private).unwrap();
        let image = dir.path().join("Foo.AppImage");
        std::fs::write(&image, b"x").unwrap();
        Rig {
            dir,
            appdir,
            home,
            private,
            image,
        }
    }

    fn real_host(home: &Path, runtime: &Path) -> Host {
        Host {
            uid: 1000,
            home: home.into(),
            runtime_dir: runtime.into(),
            env: BTreeMap::new(),
            wayland_display: None,
            x_display: None,
            xauthority: None,
            dbus_address: None,
            exists: Box::new(|p| p.exists()),
            list: Box::new(|_, _| vec![]),
        }
    }

    /// Run the rig's AppRun in a sandbox and return stdout (panics with stderr on failure).
    fn run_in(
        rig: &Rig,
        settings: &Settings,
        env: &BTreeMap<String, String>,
        args: &[String],
    ) -> std::process::Output {
        let host = real_host(&rig.home, &rig.dir.path().join("run"));
        let l = Launch {
            id: "foo",
            appdir: &rig.appdir,
            appimage: &rig.image,
            private_home: &rig.private,
            proxy_socket: Path::new("/unused"),
            app_args: args,
            env,
            cwd: None,
        };
        let s = build(settings, &host, &l).unwrap();
        Command::new(&s.argv[0])
            .args(&s.argv[1..])
            .output()
            .unwrap()
    }

    fn quiet(level: Level) -> Settings {
        Settings {
            gpu: false,
            audio: false,
            display: false,
            bus: Bus::Off,
            ..Settings::for_level(level)
        }
    }

    fn field(out: &std::process::Output, key: &str) -> String {
        let text = String::from_utf8_lossy(&out.stdout);
        text.lines()
            .find_map(|l| l.strip_prefix(&format!("{key}=")))
            .unwrap_or_else(|| {
                panic!(
                    "no {key} in output:\n{text}\nstderr: {}",
                    String::from_utf8_lossy(&out.stderr)
                )
            })
            .to_string()
    }

    const PROBE: &str = "#!/bin/sh\n\
        echo \"HOME=$HOME\"\n\
        echo \"SECRET_ENV=${SECRET_ENV:-unset}\"\n\
        echo \"SECRET_FILE=$(cat \"$REAL_SECRET\" 2>&1 | head -c 40)\"\n\
        echo \"NET=$(grep -c : /proc/net/dev)\"\n\
        echo \"PIDS=$(ls /proc | grep -c '^[0-9][0-9]*$')\"\n\
        echo \"APPDIR=$APPDIR\"\n\
        echo \"ARGS=$*\"\n\
        echo \"PWD=$(pwd)\"\n";

    #[test]
    fn strict_sandbox_really_hides_files_environment_network_and_processes() {
        if !bwrap_works() {
            eprintln!("skipped: bubblewrap is not usable here");
            return;
        }
        let r = rig(PROBE);
        let secret = r.home.join("secret.txt");
        std::fs::write(&secret, "TOP-SECRET").unwrap();
        let env: BTreeMap<String, String> =
            [("REAL_SECRET".to_string(), secret.display().to_string())].into();
        // Present in *our* environment, must not leak into the sandbox.
        // SAFETY: this test is the only one in this binary touching SECRET_ENV.
        unsafe { std::env::set_var("SECRET_ENV", "leaked") };
        let out = run_in(
            &r,
            &quiet(Level::Strict),
            &env,
            &["--a".to_string(), "b".to_string()],
        );
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );

        assert_eq!(
            field(&out, "HOME"),
            r.home.display().to_string(),
            "same path as the real home, but..."
        );
        let secret_read = field(&out, "SECRET_FILE");
        assert!(
            secret_read.starts_with("cat:") && !secret_read.contains("TOP-SECRET"),
            "...your real files are not there: {secret_read}"
        );
        assert_eq!(field(&out, "SECRET_ENV"), "unset");
        assert_eq!(
            field(&out, "NET"),
            "1",
            "only the loopback interface exists"
        );
        assert!(
            field(&out, "PIDS").parse::<u32>().unwrap() < 10,
            "own PID namespace: {}",
            field(&out, "PIDS")
        );
        assert_eq!(field(&out, "APPDIR"), "/app");
        assert_eq!(field(&out, "ARGS"), "--a b");
        assert_eq!(field(&out, "PWD"), r.home.display().to_string());
    }

    #[test]
    fn network_on_keeps_the_hosts_interfaces() {
        if !bwrap_works() {
            return;
        }
        let r = rig(PROBE);
        let env: BTreeMap<String, String> =
            [("REAL_SECRET".to_string(), "/nonexistent".to_string())].into();
        let out = run_in(&r, &quiet(Level::Standard), &env, &[]);
        let host_ifaces = std::fs::read_to_string("/proc/net/dev")
            .unwrap()
            .lines()
            .filter(|l| l.contains(':'))
            .count();
        assert_eq!(field(&out, "NET"), host_ifaces.to_string());
    }

    #[test]
    fn private_home_persists_between_runs_and_stays_off_the_real_home() {
        if !bwrap_works() {
            return;
        }
        let r = rig(
            "#!/bin/sh\nif [ -f \"$HOME/state\" ]; then echo \"STATE=$(cat $HOME/state)\"; else echo STATE=none; echo first > \"$HOME/state\"; fi\n",
        );
        let env = BTreeMap::new();
        let first = run_in(&r, &quiet(Level::Standard), &env, &[]);
        assert_eq!(field(&first, "STATE"), "none");
        let second = run_in(&r, &quiet(Level::Standard), &env, &[]);
        assert_eq!(field(&second, "STATE"), "first");
        assert!(r.private.join("state").exists());
        assert!(
            !r.home.join("state").exists(),
            "nothing was written to the real home"
        );
    }

    #[test]
    fn granted_folders_follow_their_read_write_setting() {
        if !bwrap_works() {
            return;
        }
        let r = rig(
            "#!/bin/sh\necho \"RO=$(echo x > \"$RO_DIR/new\" 2>&1 && echo wrote || echo denied)\"\necho \"RW=$(echo x > \"$RW_DIR/new\" 2>&1 && echo wrote || echo denied)\"\necho \"SEE=$(cat \"$RO_DIR/hello\")\"\n",
        );
        let ro = r.dir.path().join("ro");
        let rw = r.dir.path().join("rw");
        std::fs::create_dir(&ro).unwrap();
        std::fs::create_dir(&rw).unwrap();
        std::fs::write(ro.join("hello"), "world").unwrap();
        let env: BTreeMap<String, String> = [
            ("RO_DIR".to_string(), ro.display().to_string()),
            ("RW_DIR".to_string(), rw.display().to_string()),
        ]
        .into();
        let settings = Settings {
            paths: vec![
                PathGrant {
                    path: ro.clone(),
                    write: false,
                },
                PathGrant {
                    path: rw.clone(),
                    write: true,
                },
            ],
            ..quiet(Level::Standard)
        };
        let out = run_in(&r, &settings, &env, &[]);
        assert_eq!(field(&out, "SEE"), "world");
        assert_eq!(field(&out, "RO"), "denied");
        assert_eq!(field(&out, "RW"), "wrote");
        assert!(rw.join("new").exists() && !ro.join("new").exists());
    }

    #[test]
    fn the_sandbox_cannot_see_other_parts_of_the_system() {
        if !bwrap_works() {
            return;
        }
        let r = rig(
            "#!/bin/sh\necho \"ROOT=$(ls / | tr '\\n' ' ')\"\necho \"SHADOW=$(cat /etc/shadow 2>&1 | head -c 30)\"\necho \"VAR=$(ls /var 2>&1 | head -c 30)\"\n",
        );
        let out = run_in(&r, &quiet(Level::Strict), &BTreeMap::new(), &[]);
        let root = field(&out, "ROOT");
        for hidden in ["boot", "mnt", "srv", "opt"] {
            assert!(
                !root.split_whitespace().any(|d| d == hidden),
                "/{hidden} should not exist in the sandbox: {root}"
            );
        }
        assert!(
            field(&out, "SHADOW").contains("No such file"),
            "{}",
            field(&out, "SHADOW")
        );
        assert!(root.contains("app") && root.contains("usr") && root.contains("proc"));
    }

    #[test]
    fn exit_codes_pass_through() {
        if !bwrap_works() {
            return;
        }
        let r = rig("#!/bin/sh\nexit 7\n");
        let out = run_in(&r, &quiet(Level::Strict), &BTreeMap::new(), &[]);
        assert_eq!(out.status.code(), Some(7));
    }

    #[test]
    fn disabled_settings_refuse_to_build() {
        let env = BTreeMap::new();
        assert!(build(&Settings::default(), &host(FULL), &launch(&env, &[])).is_err());
    }
}
