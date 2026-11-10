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
    assert!(a.contains(&"--clearenv".to_string()) && a.contains(&"--die-with-parent".to_string()));
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
        "...your real files are not there (the read returned {} bytes, starting with cat: {})",
        secret_read.len(),
        secret_read.starts_with("cat:")
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
