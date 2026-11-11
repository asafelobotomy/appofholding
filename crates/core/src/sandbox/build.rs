//! Building the full bwrap command line for one launch.

use super::*;

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
