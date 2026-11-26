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

mod grants;
mod policy;
mod runtime;
