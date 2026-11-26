use super::*;
use crate::fixture;
use crate::library::AddOptions;
use crate::paths::Dirs;
use std::fs;

struct Env {
    tmp: tempfile::TempDir,
    lib: Library,
}

fn env() -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let lib = Library::open(Dirs::with_root(tmp.path()), tmp.path().join("appofholding")).unwrap();
    let p = tmp.path().join("Foo.AppImage");
    fs::write(&p, fixture::simple_appimage("Foo App", None)).unwrap();
    lib.add(&p, &AddOptions::default()).unwrap();
    Env { tmp, lib }
}

// ---- sandbox integration

use crate::sandbox::{Bus, Level};
use std::collections::BTreeMap;

fn test_host(root: &Path) -> Host {
    Host {
        uid: 1000,
        home: root.join("realhome"),
        runtime_dir: root.join("run"),
        env: BTreeMap::new(),
        wayland_display: None,
        x_display: None,
        xauthority: None,
        dbus_address: None,
        exists: Box::new(|p| p.exists()),
        list: Box::new(|_, _| vec![]),
    }
}

fn ctx(root: &Path) -> LaunchCtx {
    LaunchCtx {
        host: Some(test_host(root)),
        ..Default::default()
    }
}

fn quiet_strict() -> Settings {
    Settings {
        gpu: false,
        audio: false,
        display: false,
        bus: Bus::Off,
        ..Settings::for_level(Level::Strict)
    }
}

/// An app whose AppRun reports its environment, added to a fresh library.
fn sandboxed_env() -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let lib = Library::open(Dirs::with_root(tmp.path()), tmp.path().join("appofholding")).unwrap();
    std::fs::create_dir_all(tmp.path().join("realhome")).unwrap();
    let desktop = "[Desktop Entry]\nType=Application\nName=Probe\nExec=AppRun\n";
    let apprun = "#!/bin/sh\necho \"HOME=$HOME\"\necho \"ARGS=$*\"\necho \"NET=$(grep -c : /proc/net/dev)\"\necho hi > \"$HOME/made\"\n";
    let bytes = fixture::build_appimage(
        None,
        vec![
            fixture::Item::File("/probe.desktop", desktop.as_bytes().to_vec()),
            fixture::Item::File("/AppRun", apprun.as_bytes().to_vec()),
        ],
    );
    let p = tmp.path().join("Probe.AppImage");
    fs::write(&p, bytes).unwrap();
    lib.add(&p, &AddOptions::default()).unwrap();
    Env { tmp, lib }
}

fn bwrap_ok() -> bool {
    std::process::Command::new("bwrap")
        .args(crate::sandbox::probe_args())
        .output()
        .is_ok_and(|o| o.status.success())
}

mod hooks_bus;
mod profiles;
