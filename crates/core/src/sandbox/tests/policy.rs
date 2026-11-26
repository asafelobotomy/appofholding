use super::*;

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
        // Spellings of the same places that a plain prefix test would miss.
        "/usr/../run/user/1000",
        "/home/u/../../proc",
        "//run/user/1000",
        "/./run",
        "//",
        "/sys/",
    ] {
        assert!(mk(bad).validate().is_err(), "{bad} should be refused");
    }
    // `/running` is not `/run`.
    assert!(mk("/running/x").validate().is_ok());
}

#[test]
fn a_shared_symlink_into_a_forbidden_tree_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let link = tmp.path().join("innocent");
    std::os::unix::fs::symlink("/proc/self", &link).unwrap();
    let s = Settings {
        paths: vec![PathGrant {
            path: link.clone(),
            write: false,
        }],
        ..Settings::for_level(Level::Standard)
    };
    assert!(
        s.validate().is_err(),
        "{} should be refused",
        link.display()
    );
    let to_root = tmp.path().join("root");
    std::os::unix::fs::symlink("/", &to_root).unwrap();
    let s = Settings {
        paths: vec![PathGrant {
            path: to_root,
            write: true,
        }],
        ..Settings::for_level(Level::Standard)
    };
    assert!(s.validate().is_err());
}

#[test]
fn the_filtered_bus_lets_tray_icons_register() {
    let env = BTreeMap::new();
    let s = build(
        &Settings::for_level(Level::Standard),
        &host(FULL),
        &launch(&env, &[]),
    )
    .unwrap();
    let h = s.helper.unwrap();
    assert!(
        h.argv
            .contains(&"--talk=org.kde.StatusNotifierWatcher".to_string())
    );
    assert!(
        h.argv
            .contains(&"--own=org.kde.StatusNotifierItem-2-1".to_string())
    );
    // Exact names only: never a wildcard over every tray icon on the bus.
    assert!(
        !h.argv
            .iter()
            .any(|a| a.starts_with("--own=org.kde.StatusNotifierItem") && a.ends_with('*'))
    );
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
