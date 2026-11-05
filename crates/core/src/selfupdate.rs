//! appOFholding updating itself.
//!
//! Where the program came from decides what "update" can mean:
//!
//! * **pacman package** (files owned by the package manager): download the release's package,
//!   verify its checksum, and hand over the exact `run0 pacman -U` command. We never overwrite
//!   files that pacman owns.
//! * **AppImage** (`$APPIMAGE` is set): download the new AppImage, verify it, and replace the
//!   running file in place, keeping the previous one next to it.
//! * **user-local or unknown installs**: tell the user how to update; nothing is changed.
//!
//! Release information comes from this project's GitHub releases.

use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::assets::{self, Arch, Asset, Selection};
use crate::download;
use crate::error::{Error, Result};
use crate::http::Http;
use crate::library::{inspect, write_atomic};
use crate::notify::Notifier;
use crate::paths::Dirs;
use crate::providers;
use crate::source::{Provider, Source};
use crate::version;

pub const REPO_OWNER: &str = "asafelobotomy";
pub const REPO_NAME: &str = "appofholding";
/// How long a cached "latest version" answer is trusted before asking GitHub again.
pub const CACHE_SECONDS: i64 = 6 * 3600;

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallKind {
    /// Files owned by this pacman package.
    Pacman {
        package: String,
    },
    /// Running from this AppImage file.
    AppImage {
        path: PathBuf,
    },
    /// Installed somewhere under the home folder (for example `make install PREFIX=~/.local`).
    UserLocal {
        path: PathBuf,
    },
    Unknown,
}

impl InstallKind {
    pub fn label(&self) -> &'static str {
        match self {
            InstallKind::Pacman { .. } => "pacman",
            InstallKind::AppImage { .. } => "appimage",
            InstallKind::UserLocal { .. } => "local",
            InstallKind::Unknown => "unknown",
        }
    }
}

/// Decide how this copy was installed. `owner_of` asks the package manager who owns a file.
pub fn detect_install(
    exe: &Path,
    appimage_env: Option<&Path>,
    home: Option<&Path>,
    owner_of: &dyn Fn(&Path) -> Option<String>,
) -> InstallKind {
    if let Some(p) = appimage_env.filter(|p| p.is_file()) {
        return InstallKind::AppImage {
            path: p.to_path_buf(),
        };
    }
    if let Some(package) = owner_of(exe) {
        return InstallKind::Pacman { package };
    }
    if home.is_some_and(|h| exe.starts_with(h)) {
        return InstallKind::UserLocal {
            path: exe.to_path_buf(),
        };
    }
    InstallKind::Unknown
}

/// The install kind of the running program.
pub fn detect_current() -> InstallKind {
    let exe = std::env::current_exe().unwrap_or_default();
    let exe = fs::canonicalize(&exe).unwrap_or(exe);
    let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from);
    let home = std::env::var_os("HOME").map(PathBuf::from);
    detect_install(&exe, appimage.as_deref(), home.as_deref(), &|p| {
        let out = Command::new("pacman").arg("-Qqo").arg(p).output().ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
    })
}

#[derive(Debug, Clone)]
pub struct SelfStatus {
    pub current: String,
    pub latest: String,
    pub update_available: bool,
    pub release_url: String,
    pub notes: Option<String>,
    pub install: InstallKind,
    /// The release file that fits this install kind and architecture, if any.
    pub asset: Option<Asset>,
    /// Answered from the local cache without asking GitHub.
    pub from_cache: bool,
}

fn arch_name(a: Arch) -> &'static str {
    match a {
        Arch::X86_64 => "x86_64",
        Arch::Aarch64 => "aarch64",
        Arch::Armv7 => "armv7h",
        Arch::X86 => "i686",
    }
}

fn asset_pattern(kind: &InstallKind, arch: Arch) -> Option<String> {
    match kind {
        InstallKind::Pacman { .. } => {
            Some(format!("appofholding-*-{}.pkg.tar.zst", arch_name(arch)))
        }
        InstallKind::AppImage { .. } => {
            Some(format!("appofholding-*{}*.AppImage", arch_name(arch)))
        }
        _ => None,
    }
}

fn release_url(tag: &str) -> String {
    format!("https://github.com/{REPO_OWNER}/{REPO_NAME}/releases/tag/{tag}")
}

fn is_newer(current: &str, latest: &str) -> bool {
    version::compare(current, latest) == Some(Ordering::Less)
}

/// Ask GitHub for the newest release and compare it with the running version.
pub fn check(http: &dyn Http, install: InstallKind, arch: Arch) -> Result<SelfStatus> {
    let src = Source::new(Provider::GitHub {
        owner: REPO_OWNER.into(),
        repo: REPO_NAME.into(),
    });
    let rel = providers::fetch_release(http, &src)?;
    let latest = rel.tag.trim_start_matches('v').to_string();
    let asset = match asset_pattern(&install, arch) {
        Some(p) => match assets::select(&rel.assets, Some(&p), arch) {
            Selection::One(a) => Some(a),
            _ => None,
        },
        None => None,
    };
    Ok(SelfStatus {
        current: current_version().into(),
        update_available: is_newer(current_version(), &latest),
        release_url: release_url(&rel.tag),
        latest,
        notes: rel.notes,
        install,
        asset,
        from_cache: false,
    })
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelfState {
    pub last_checked: i64,
    pub latest: Option<String>,
    /// The version the user was last told about (so notifications fire once).
    pub notified: Option<String>,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn state_path(dirs: &Dirs) -> PathBuf {
    dirs.app_data_dir().join("self-update.json")
}

pub fn load_state(dirs: &Dirs) -> SelfState {
    fs::read_to_string(state_path(dirs))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_state(dirs: &Dirs, s: &SelfState) -> Result<()> {
    write_atomic(
        &state_path(dirs),
        serde_json::to_string_pretty(s)
            .unwrap_or_default()
            .as_bytes(),
    )
}

/// Like [`check`], but trusts a recent answer instead of asking GitHub every time.
/// `force` always asks.
pub fn check_cached(
    http: &dyn Http,
    dirs: &Dirs,
    install: InstallKind,
    arch: Arch,
    force: bool,
) -> Result<SelfStatus> {
    let state = load_state(dirs);
    if !force
        && now() - state.last_checked < CACHE_SECONDS
        && let Some(latest) = state.latest.clone()
    {
        return Ok(SelfStatus {
            current: current_version().into(),
            update_available: is_newer(current_version(), &latest),
            release_url: release_url(&format!("v{latest}")),
            latest,
            notes: None,
            install,
            asset: None,
            from_cache: true,
        });
    }
    let status = check(http, install, arch)?;
    save_state(
        dirs,
        &SelfState {
            last_checked: now(),
            latest: Some(status.latest.clone()),
            notified: state.notified,
        },
    )?;
    Ok(status)
}

/// For the background updater: tell the user about a new release once. Returns the message if
/// a notification was sent.
pub fn notify_if_new(
    http: &dyn Http,
    notifier: &dyn Notifier,
    dirs: &Dirs,
    install: InstallKind,
    arch: Arch,
) -> Result<Option<String>> {
    let status = check_cached(http, dirs, install, arch, false)?;
    let mut state = load_state(dirs);
    if !status.update_available || state.notified.as_deref() == Some(status.latest.as_str()) {
        return Ok(None);
    }
    let how = match &status.install {
        InstallKind::Pacman { .. } => {
            "Run `appofholding self update --install` to update.".to_string()
        }
        InstallKind::AppImage { .. } => "Run `appofholding self update` to update.".to_string(),
        _ => format!("See {}", status.release_url),
    };
    let body = format!(
        "Version {} is available (you have {}). {how}",
        status.latest, status.current
    );
    notifier.notify("appOFholding: update available", &body);
    state.notified = Some(status.latest.clone());
    save_state(dirs, &state)?;
    Ok(Some(body))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Applied {
    AlreadyCurrent,
    /// The running AppImage was replaced; the old one is kept at `backup`.
    Replaced {
        to: String,
        path: PathBuf,
        backup: PathBuf,
    },
    /// The package is downloaded and verified. Run `command` to install it.
    PackageReady {
        to: String,
        file: PathBuf,
        command: Vec<String>,
    },
    /// Nothing was changed; do this instead.
    Manual {
        to: String,
        instructions: String,
    },
}

/// Carry out the update as far as this install kind allows (see the module documentation).
pub fn apply(
    http: &dyn Http,
    status: &SelfStatus,
    work_dir: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<Applied> {
    if !status.update_available {
        return Ok(Applied::AlreadyCurrent);
    }
    let to = status.latest.clone();
    let need_asset = || {
        status.asset.clone().ok_or_else(|| {
            Error::Source(format!(
                "release {to} has no file for this installation and architecture; see {}",
                status.release_url
            ))
        })
    };
    match &status.install {
        InstallKind::Pacman { .. } => {
            let asset = need_asset()?;
            let (file, _) = download::download_asset(http, &asset, work_dir, progress)?;
            let command = vec![
                "run0".to_string(),
                "pacman".into(),
                "-U".into(),
                file.display().to_string(),
            ];
            Ok(Applied::PackageReady { to, file, command })
        }
        InstallKind::AppImage { path } => {
            let asset = need_asset()?;
            let (new_file, _) = download::download_asset(http, &asset, work_dir, progress)?;
            if let Err(e) = inspect(&new_file) {
                let _ = fs::remove_file(&new_file);
                return Err(Error::Source(format!(
                    "the downloaded file is not a usable AppImage ({e}); nothing was changed"
                )));
            }
            let dir = path
                .parent()
                .ok_or_else(|| Error::Other("the AppImage has no folder".into()))?;
            let stage = dir.join(format!(
                ".{}.new",
                path.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            ));
            let backup = path.with_extension("AppImage.previous");
            if fs::rename(&new_file, &stage).is_err() {
                fs::copy(&new_file, &stage).map_err(Error::io(&stage))?;
                let _ = fs::remove_file(&new_file);
            }
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(&stage)
                    .map_err(Error::io(&stage))?
                    .permissions();
                perms.set_mode(perms.mode() | 0o755);
                fs::set_permissions(&stage, perms).map_err(Error::io(&stage))?;
            }
            // Keep the old file, then swap atomically: the path always names a working AppImage.
            let _ = fs::remove_file(&backup);
            if fs::hard_link(path, &backup).is_err() {
                fs::copy(path, &backup).map_err(Error::io(&backup))?;
            }
            if let Err(e) = fs::rename(&stage, path) {
                let _ = fs::remove_file(&stage);
                return Err(Error::io(path)(e));
            }
            Ok(Applied::Replaced {
                to,
                path: path.clone(),
                backup,
            })
        }
        InstallKind::UserLocal { path } => Ok(Applied::Manual {
            to,
            instructions: format!(
                "appOFholding is installed under your home folder ({}). Download the new release from {} and run `make install PREFIX=~/.local` again, or install the package for your system.",
                path.display(),
                status.release_url
            ),
        }),
        InstallKind::Unknown => Ok(Applied::Manual {
            to,
            instructions: format!(
                "Update appOFholding with the tool you installed it with, or download it from {}.",
                status.release_url
            ),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture;
    use crate::http::fake::FakeHttp;
    use crate::notify::Recorder;
    use sha2::{Digest, Sha256};

    const API: &str = "https://api.github.com/repos/asafelobotomy/appofholding/releases/latest";

    fn release(tag: &str, pkg: &[u8], appimage: &[u8]) -> String {
        let sha = |b: &[u8]| hex::encode(Sha256::digest(b));
        format!(
            r#"{{"tag_name":"{tag}","prerelease":false,"draft":false,"body":"Notes","assets":[
              {{"name":"appofholding-9.9.9-1-x86_64.pkg.tar.zst","browser_download_url":"https://dl/pkg","size":{},"digest":"sha256:{}"}},
              {{"name":"appofholding-9.9.9-aarch64.pkg.tar.zst","browser_download_url":"https://dl/pkg-arm"}},
              {{"name":"appofholding-9.9.9-x86_64.AppImage","browser_download_url":"https://dl/img","size":{},"digest":"sha256:{}"}}]}}"#,
            pkg.len(),
            sha(pkg),
            appimage.len(),
            sha(appimage)
        )
    }

    fn pacman() -> InstallKind {
        InstallKind::Pacman {
            package: "appofholding".into(),
        }
    }

    fn dirs() -> (tempfile::TempDir, Dirs) {
        let tmp = tempfile::tempdir().unwrap();
        let d = Dirs::with_root(tmp.path());
        (tmp, d)
    }

    #[test]
    fn install_kind_detection() {
        let none = |_: &Path| None::<String>;
        let owned = |_: &Path| Some("appofholding".to_string());
        let tmp = tempfile::tempdir().unwrap();
        let img = tmp.path().join("A.AppImage");
        fs::write(&img, b"x").unwrap();
        let exe = Path::new("/usr/bin/appofholding");
        assert_eq!(
            detect_install(exe, Some(&img), None, &owned),
            InstallKind::AppImage { path: img.clone() }
        );
        assert_eq!(detect_install(exe, None, None, &owned), pacman());
        assert_eq!(
            detect_install(
                Path::new("/home/u/.local/bin/appofholding"),
                None,
                Some(Path::new("/home/u")),
                &none
            ),
            InstallKind::UserLocal {
                path: "/home/u/.local/bin/appofholding".into()
            }
        );
        assert_eq!(
            detect_install(exe, None, Some(Path::new("/home/u")), &none),
            InstallKind::Unknown
        );
        // A stale $APPIMAGE that does not exist is ignored.
        assert_eq!(
            detect_install(exe, Some(Path::new("/nope.AppImage")), None, &owned),
            pacman()
        );
    }

    #[test]
    fn reports_newer_older_and_equal_releases_and_picks_the_right_file() {
        let newer = FakeHttp::default().json(API, &release("v99.0.0", b"pkg", b"img"));
        let s = check(&newer, pacman(), Arch::X86_64).unwrap();
        assert!(s.update_available && s.latest == "99.0.0" && s.current == current_version());
        assert_eq!(
            s.asset.unwrap().name,
            "appofholding-9.9.9-1-x86_64.pkg.tar.zst"
        );
        assert_eq!(
            s.release_url,
            "https://github.com/asafelobotomy/appofholding/releases/tag/v99.0.0"
        );

        let img = check(
            &newer,
            InstallKind::AppImage { path: "/a".into() },
            Arch::X86_64,
        )
        .unwrap();
        assert_eq!(
            img.asset.unwrap().name,
            "appofholding-9.9.9-x86_64.AppImage"
        );
        assert!(
            check(&newer, InstallKind::Unknown, Arch::X86_64)
                .unwrap()
                .asset
                .is_none()
        );
        assert_eq!(
            check(&newer, pacman(), Arch::Aarch64)
                .unwrap()
                .asset
                .unwrap()
                .name,
            "appofholding-9.9.9-aarch64.pkg.tar.zst"
        );

        let same = FakeHttp::default().json(
            API,
            &release(&format!("v{}", current_version()), b"p", b"i"),
        );
        assert!(
            !check(&same, pacman(), Arch::X86_64)
                .unwrap()
                .update_available
        );
        let older = FakeHttp::default().json(API, &release("v0.0.1", b"p", b"i"));
        assert!(
            !check(&older, pacman(), Arch::X86_64)
                .unwrap()
                .update_available,
            "never offers a downgrade"
        );
    }

    #[test]
    fn cached_checks_do_not_ask_github_again_until_forced() {
        let (_t, d) = dirs();
        let http = FakeHttp::default().json(API, &release("v99.0.0", b"p", b"i"));
        assert!(
            !check_cached(&http, &d, pacman(), Arch::X86_64, false)
                .unwrap()
                .from_cache
        );
        let again = check_cached(&http, &d, pacman(), Arch::X86_64, false).unwrap();
        assert!(again.from_cache && again.update_available && again.latest == "99.0.0");
        assert_eq!(http.requests.lock().unwrap().len(), 1);
        assert!(
            !check_cached(&http, &d, pacman(), Arch::X86_64, true)
                .unwrap()
                .from_cache
        );
        assert_eq!(http.requests.lock().unwrap().len(), 2);
    }

    #[test]
    fn the_background_run_announces_a_release_once() {
        let (_t, d) = dirs();
        let http = FakeHttp::default().json(API, &release("v99.0.0", b"p", b"i"));
        let notes = Recorder::default();
        let msg = notify_if_new(&http, &notes, &d, pacman(), Arch::X86_64)
            .unwrap()
            .unwrap();
        assert!(
            msg.contains("99.0.0") && msg.contains("self update --install"),
            "{msg}"
        );
        assert!(
            notify_if_new(&http, &notes, &d, pacman(), Arch::X86_64)
                .unwrap()
                .is_none()
        );
        assert_eq!(notes.0.borrow().len(), 1);
        // A newer release later is announced again.
        let newer = FakeHttp::default().json(API, &release("v100.0.0", b"p", b"i"));
        let mut st = load_state(&d);
        st.last_checked = 0;
        save_state(&d, &st).unwrap();
        assert!(
            notify_if_new(&newer, &notes, &d, pacman(), Arch::X86_64)
                .unwrap()
                .is_some()
        );
        assert_eq!(notes.0.borrow().len(), 2);
        // Nothing to say when up to date.
        let (_t2, d2) = dirs();
        let same = FakeHttp::default().json(
            API,
            &release(&format!("v{}", current_version()), b"p", b"i"),
        );
        assert!(
            notify_if_new(&same, &notes, &d2, pacman(), Arch::X86_64)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn pacman_installs_get_a_verified_package_and_the_command_to_install_it() {
        let (tmp, _d) = dirs();
        let pkg = b"fake package bytes".to_vec();
        let http = FakeHttp::default()
            .json(API, &release("v99.0.0", &pkg, b"i"))
            .bytes("https://dl/pkg", pkg.clone());
        let status = check(&http, pacman(), Arch::X86_64).unwrap();
        let out = apply(&http, &status, &tmp.path().join("work"), &mut |_, _| {}).unwrap();
        let Applied::PackageReady { to, file, command } = out else {
            panic!("expected a package")
        };
        assert_eq!(to, "99.0.0");
        assert_eq!(fs::read(&file).unwrap(), pkg);
        assert_eq!(&command[..3], ["run0", "pacman", "-U"]);
        assert_eq!(command[3], file.display().to_string());

        // A corrupted download is rejected and leaves no file behind.
        let bad = FakeHttp::default()
            .json(API, &release("v99.0.0", &pkg, b"i"))
            .bytes("https://dl/pkg", b"tampered".to_vec());
        let status = check(&bad, pacman(), Arch::X86_64).unwrap();
        let err = apply(&bad, &status, &tmp.path().join("work2"), &mut |_, _| {}).unwrap_err();
        assert!(
            matches!(err, Error::Checksum { .. }) || err.to_string().contains("wrong size"),
            "{err}"
        );
    }

    #[test]
    fn appimage_installs_are_replaced_in_place_with_a_backup() {
        let (tmp, _d) = dirs();
        let current = tmp.path().join("appofholding-x86_64.AppImage");
        let old = fixture::versioned_appimage("appOFholding", "0.0.1", None);
        fs::write(&current, &old).unwrap();
        let new = fixture::versioned_appimage("appOFholding", "99.0.0", None);
        let http = FakeHttp::default()
            .json(API, &release("v99.0.0", b"p", &new))
            .bytes("https://dl/img", new.clone());
        let install = InstallKind::AppImage {
            path: current.clone(),
        };
        let status = check(&http, install, Arch::X86_64).unwrap();
        let Applied::Replaced { to, path, backup } =
            apply(&http, &status, &tmp.path().join("work"), &mut |_, _| {}).unwrap()
        else {
            panic!()
        };
        assert_eq!(to, "99.0.0");
        assert_eq!(fs::read(&path).unwrap(), new);
        assert_eq!(
            fs::read(&backup).unwrap(),
            old,
            "the previous version is kept"
        );
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o111,
            0,
            "still executable"
        );

        // Something that is not an AppImage never replaces the running one.
        let junk = b"<html>oops</html>".to_vec();
        let http = FakeHttp::default()
            .json(API, &release("v99.0.0", b"p", &junk))
            .bytes("https://dl/img", junk);
        let status = check(
            &http,
            InstallKind::AppImage {
                path: current.clone(),
            },
            Arch::X86_64,
        )
        .unwrap();
        let before = fs::read(&current).unwrap();
        let err = apply(&http, &status, &tmp.path().join("work3"), &mut |_, _| {})
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("not a usable AppImage") && err.contains("nothing was changed"),
            "{err}"
        );
        assert_eq!(fs::read(&current).unwrap(), before);
    }

    #[test]
    fn local_and_unknown_installs_get_instructions_and_nothing_changes() {
        let (tmp, _d) = dirs();
        let http = FakeHttp::default().json(API, &release("v99.0.0", b"p", b"i"));
        for kind in [
            InstallKind::UserLocal {
                path: "/home/u/.local/bin/appofholding".into(),
            },
            InstallKind::Unknown,
        ] {
            let status = check(&http, kind, Arch::X86_64).unwrap();
            let Applied::Manual { instructions, .. } =
                apply(&http, &status, tmp.path(), &mut |_, _| {}).unwrap()
            else {
                panic!()
            };
            assert!(
                instructions.contains("releases/tag/v99.0.0"),
                "{instructions}"
            );
        }
        // Already current.
        let same = FakeHttp::default().json(
            API,
            &release(&format!("v{}", current_version()), b"p", b"i"),
        );
        let status = check(&same, pacman(), Arch::X86_64).unwrap();
        assert_eq!(
            apply(&same, &status, tmp.path(), &mut |_, _| {}).unwrap(),
            Applied::AlreadyCurrent
        );
    }

    #[test]
    fn a_release_without_a_matching_file_explains_itself() {
        let (tmp, _d) = dirs();
        let http = FakeHttp::default().json(
            API,
            r#"{"tag_name":"v99.0.0","prerelease":false,"draft":false,"assets":[]}"#,
        );
        let status = check(&http, pacman(), Arch::X86_64).unwrap();
        let err = apply(&http, &status, tmp.path(), &mut |_, _| {})
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("no file for this installation") && err.contains("releases/tag/v99.0.0"),
            "{err}"
        );
    }
}
