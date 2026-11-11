//! Building blocks of the bwrap command line: base binds, D-Bus names and sockets.

use super::*;

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
pub(super) const RESERVED_BUS_PREFIXES: &[&str] = &[
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
pub(super) fn ownable_bus_name(n: &str) -> bool {
    valid_bus_name(n)
        && n.split('.').count() >= 3
        && !RESERVED_BUS_PREFIXES
            .iter()
            .any(|p| n.starts_with(p) || format!("{n}.").starts_with(p))
}

pub(super) fn valid_bus_name(n: &str) -> bool {
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

pub(super) fn push(v: &mut Vec<String>, items: &[&str]) {
    v.extend(items.iter().map(|s| s.to_string()));
}

pub(super) fn x11_socket(display: &str) -> Option<PathBuf> {
    // ":1", ":1.0" and "unix:1" are local displays; "host:N" is a network display.
    let rest = display
        .strip_prefix("unix:")
        .or_else(|| display.strip_prefix(':'))?;
    let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    (!num.is_empty()).then(|| PathBuf::from(format!("/tmp/.X11-unix/X{num}")))
}

pub(super) fn dbus_path(addr: &str) -> Option<PathBuf> {
    addr.split(';').find_map(|a| {
        a.strip_prefix("unix:")?
            .split(',')
            .find_map(|kv| kv.strip_prefix("path=").map(PathBuf::from))
    })
}

/// Files and folders under `/etc` that programs need to start, resolve names, and find fonts and
/// certificates. Everything else in `/etc` stays out.
pub(super) const ETC: &[&str] = &[
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
