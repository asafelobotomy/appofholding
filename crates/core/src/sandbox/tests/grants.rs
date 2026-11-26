use super::*;

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
