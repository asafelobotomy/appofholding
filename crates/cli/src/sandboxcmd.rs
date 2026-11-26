//! `appofholding sandbox`: show, change and check an app's sandbox.

use super::*;

pub(crate) fn onoff(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

pub(crate) fn on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
}

pub(crate) fn sandbox_cmd(cmd: SandboxCmd) -> Result<()> {
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

pub(crate) fn print_warnings(lib: &Library) {
    for w in lib.take_warnings() {
        eprintln!("warning: {w}");
    }
}
