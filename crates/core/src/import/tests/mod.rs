use super::*;
use crate::fixture;
use crate::paths::Dirs;

struct Env {
    tmp: tempfile::TempDir,
    home: PathBuf,
    lib: Library,
}

fn write_image(path: &Path, name: &str, upd: Option<&str>) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, fixture::simple_appimage(name, upd)).unwrap();
}

fn entry(lib: &Library, file: &str, body: &str) -> PathBuf {
    let p = lib.dirs.applications_dir().join(file);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, format!("[Desktop Entry]\nType=Application\n{body}\n")).unwrap();
    p
}

/// A pretend home folder with every kind of AppImage this feature has to handle.
fn env() -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let lib = Library::open(
        Dirs::with_root(&tmp.path().join("lib")),
        tmp.path().join("appofholding"),
    )
    .unwrap();
    // loose, in a folder people keep apps in / in Downloads
    write_image(&home.join("AppImages/Loose.AppImage"), "Loose App", None);
    write_image(
        &home.join("Downloads/Dl.AppImage"),
        "Download App",
        Some("gh-releases-zsync|o|dl|latest|Dl-*.AppImage.zsync"),
    );
    // integrated by Gear Lever
    let gl = home.join("AppImages/gear.appimage");
    write_image(&gl, "Gear App", None);
    entry(
        &lib,
        "gear.desktop",
        &format!(
            "Name=Gear App\nTryExec={0}\nExec=env DESKTOPINTEGRATION=1 {0} %U",
            gl.display()
        ),
    );
    // kept and updated by the app itself, in a folder that is not an AppImage folder
    let owned = home.join(".local/share/foo/appimage");
    write_image(
        &owned,
        "Foo Owned",
        Some("gh-releases-zsync|o|foo|latest|Foo-*.AppImage.zsync"),
    );
    entry(
        &lib,
        "foo.desktop",
        &format!(
            "Name=Foo\nX-Foo-Integrator=appimage\nExec={} -- %U",
            owned.display()
        ),
    );
    // not an AppImage / hidden / ours: none of these may be offered
    fs::write(
        home.join("AppImages/Fake.AppImage"),
        "text, not an AppImage",
    )
    .unwrap();
    write_image(
        &home.join("AppImages/.hidden/Skip.AppImage"),
        "Hidden",
        None,
    );
    entry(
        &lib,
        "appofholding-ours.desktop",
        &format!("Name=Ours\nExec={}", gl.display()),
    );
    Env { tmp, home, lib }
}

fn scan_all(e: &Env) -> Scan {
    scan(&e.lib, &ScanOptions::new(&e.home)).unwrap()
}

fn by_name<'a>(s: &'a Scan, name: &str) -> &'a Found {
    s.found.iter().find(|f| f.name == name).unwrap_or_else(|| {
        panic!(
            "{name} not found in {:?}",
            s.found.iter().map(|f| &f.name).collect::<Vec<_>>()
        )
    })
}

/// Every file (path + size) under a folder, to prove nothing changed.
fn snapshot(root: &Path) -> Vec<(PathBuf, u64)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if !p.ends_with("db.sqlite") && !p.to_string_lossy().contains("sqlite-") {
                out.push((p.clone(), fs::metadata(&p).map(|m| m.len()).unwrap_or(0)));
            }
        }
    }
    out.sort();
    out
}

mod conflicts;
mod plan;
