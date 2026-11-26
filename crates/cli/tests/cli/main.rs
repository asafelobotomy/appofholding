use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use appofholding_core::fixture;

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_appofholding"))
        .env("APPOFHOLDING_ROOT", root)
        .args(args)
        .output()
        .unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn add_fixture(root: &Path) -> std::path::PathBuf {
    let img = root.join("Foo.AppImage");
    fs::write(&img, fixture::simple_appimage("Foo App", None)).unwrap();
    let o = run(root, &["add", img.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    root.join("Applications/Foo.AppImage")
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn bwrap_usable() -> bool {
    Command::new("bwrap")
        .args([
            "--unshare-all",
            "--ro-bind",
            "/usr",
            "/usr",
            "--symlink",
            "usr/bin",
            "/bin",
            "--symlink",
            "usr/lib",
            "/lib",
            "--symlink",
            "usr/lib",
            "/lib64",
            "--proc",
            "/proc",
            "--dev",
            "/dev",
            "/bin/true",
        ])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// A probe app: AppRun prints what the sandbox lets it see.
fn add_probe(root: &Path) {
    use appofholding_core::fixture::{Item, build_appimage};
    let desktop = "[Desktop Entry]\nType=Application\nName=Probe\nExec=AppRun\n";
    let apprun = "#!/bin/sh\necho \"HOME_FILES=$(find \"$HOME\" -maxdepth 1 -type f | wc -l)\"\necho \"NET=$(grep -c : /proc/net/dev)\"\necho \"ARGS=$*\"\ntouch \"$HOME/created-by-app\"\n";
    let img = build_appimage(
        None,
        vec![
            Item::File("/probe.desktop", desktop.as_bytes().to_vec()),
            Item::File("/AppRun", apprun.as_bytes().to_vec()),
        ],
    );
    let p = root.join("Probe.AppImage");
    fs::write(&p, img).unwrap();
    let o = run(root, &["add", p.to_str().unwrap()]);
    assert!(o.status.success(), "{}", err(&o));
}

/// Like `run`, but with a pretend home folder to scan.
fn run_home(root: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_appofholding"))
        .env("APPOFHOLDING_ROOT", root)
        .env("HOME", home)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}

/// A home folder with a loose AppImage, one integrated by Gear Lever, one in Downloads.
fn import_fixture(root: &Path) -> std::path::PathBuf {
    let home = root.join("home");
    fs::create_dir_all(home.join("AppImages")).unwrap();
    fs::create_dir_all(home.join("Downloads")).unwrap();
    fs::write(
        home.join("AppImages/Loose.AppImage"),
        fixture::simple_appimage("Loose App", None),
    )
    .unwrap();
    let gear = home.join("AppImages/gear.appimage");
    fs::write(
        &gear,
        fixture::simple_appimage(
            "Gear App",
            Some("gh-releases-zsync|o|gear|latest|Gear-*.AppImage.zsync"),
        ),
    )
    .unwrap();
    fs::write(
        home.join("Downloads/Dl.AppImage"),
        fixture::simple_appimage("Download App", None),
    )
    .unwrap();
    let apps = root.join("data/applications");
    fs::create_dir_all(&apps).unwrap();
    fs::write(
        apps.join("gear.desktop"),
        format!("[Desktop Entry]\nType=Application\nName=Gear App\nExec=env DESKTOPINTEGRATION=1 {0} %U\nTryExec={0}\n", gear.display()),
    )
    .unwrap();
    home
}

fn json_of(o: &Output) -> serde_json::Value {
    assert!(o.status.success(), "{}", err(o));
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", stdout(o)))
}

fn failed_json(o: &Output) -> serde_json::Value {
    assert!(!o.status.success(), "{}", stdout(o));
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", stdout(o)))
}

fn lines_of(o: &Output) -> Vec<serde_json::Value> {
    stdout(o)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{e}: {l}")))
        .collect()
}

mod export_import;
mod json_plans;
mod list_config;
mod run;
mod sandbox;
