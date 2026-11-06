//! Suggestions: small, specific improvements for an app that is set up in a way that works less
//! well than it could, each a single reversible change the user can accept or dismiss.
//!
//! Nothing here changes anything by itself. [`Library::suggestions`] only looks (at the AppImage,
//! the app's settings and a few facts about this computer); [`Library::apply_suggestion`] makes
//! the one change a suggestion describes when the user asks for it.

use std::fs;
use std::path::Path;

use crate::error::{Error, Result};
use crate::image::AppImage;
use crate::launch::DEFAULT_PROFILE;
use crate::library::{Library, inspect};

/// Setting: the user let a Chromium/Electron app use its own sandbox (its `--no-sandbox` is
/// dropped from the menu entry).
pub const CHROMIUM_SANDBOX: &str = "chromium-sandbox";

/// Old versions are only worth mentioning once they take real space.
const OLD_VERSIONS_THRESHOLD: u64 = 200 * 1024 * 1024;

/// Facts about this computer that decide which suggestions make sense.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Host {
    /// The desktop session is Wayland.
    pub wayland: bool,
    /// Unprivileged programs may create user namespaces, which Chromium's sandbox needs when it
    /// cannot use its setuid helper (it never can inside an AppImage).
    pub user_namespaces: bool,
    /// `libfuse.so.2` is installed (older AppImage runtimes need it to mount themselves).
    pub libfuse2: bool,
}

impl Host {
    pub fn detect() -> Self {
        let read = |p: &str| fs::read_to_string(p).ok().map(|s| s.trim().to_string());
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty())
            || std::env::var("XDG_SESSION_TYPE").is_ok_and(|v| v == "wayland");
        // Debian's old switch, Ubuntu's AppArmor restriction (24.04+), and the general limit.
        let user_namespaces = read("/proc/sys/kernel/unprivileged_userns_clone")
            .is_none_or(|v| v == "1")
            && read("/proc/sys/kernel/apparmor_restrict_unprivileged_userns")
                .is_none_or(|v| v == "0")
            && read("/proc/sys/user/max_user_namespaces")
                .is_none_or(|v| v.parse::<u64>().is_ok_and(|n| n > 0));
        let libfuse2 = fs::read("/etc/ld.so.cache").is_ok_and(|c| contains(&c, b"libfuse.so.2"))
            || [
                "/usr/lib/libfuse.so.2",
                "/usr/lib64/libfuse.so.2",
                "/lib64/libfuse.so.2",
                "/usr/lib/x86_64-linux-gnu/libfuse.so.2",
                "/lib/x86_64-linux-gnu/libfuse.so.2",
                "/usr/lib/aarch64-linux-gnu/libfuse.so.2",
            ]
            .iter()
            .any(|p| Path::new(p).exists());
        Host {
            wayland,
            user_namespaces,
            libfuse2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub key: String,
    pub title: String,
    pub detail: String,
    /// Label for the button that applies it.
    pub action: String,
    /// A note about a change made earlier is not dismissed, only undone.
    pub dismissible: bool,
}

/// What the AppImage itself tells us (reading it, never running it).
struct Traits {
    chromium: bool,
    own_no_sandbox: bool,
    needs_libfuse2: bool,
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn traits(path: &Path) -> Result<Traits> {
    let ins = inspect(path)?;
    let img = &ins.image;
    let chromium = is_chromium(img);
    let own_no_sandbox = ins
        .entry
        .main("Exec")
        .map(crate::desktop::split_exec)
        .is_some_and(|args| args.iter().any(|a| a == "--no-sandbox"));
    // The classic runtime loads libfuse.so.2 when it starts; newer static runtimes carry their
    // own FUSE code. The runtime is everything before the squashfs payload.
    let runtime = {
        use std::io::Read;
        let mut buf = vec![0u8; img.offset.min(4 << 20) as usize];
        fs::File::open(path)
            .and_then(|mut f| f.read_exact(&mut buf))
            .map_err(Error::io(path))?;
        buf
    };
    Ok(Traits {
        chromium,
        own_no_sandbox,
        needs_libfuse2: contains(&runtime, b"libfuse.so.2"),
    })
}

/// Chromium or Electron, at the top level or in `opt/<name>/` (the two layouts in use).
fn is_chromium(img: &AppImage) -> bool {
    let marks = |dir: &str| {
        let names: Vec<String> = img.list_dir(dir).into_iter().map(|(n, _)| n).collect();
        names.iter().any(|n| n == "chrome-sandbox")
            || (names.iter().any(|n| n == "resources")
                && img.exists(&format!("{dir}/resources/app.asar")))
    };
    marks("/")
        || img
            .list_dir("/opt")
            .into_iter()
            .filter(|(_, is_dir)| *is_dir)
            .any(|(n, _)| marks(&format!("/opt/{n}")))
}

fn mib(bytes: u64) -> u64 {
    bytes.div_ceil(1024 * 1024)
}

impl Library {
    /// Suggestions for one app on this computer, most useful first. Reads the AppImage, so call
    /// it off the interface thread for big apps.
    pub fn suggestions(&self, id: &str, host: &Host) -> Result<Vec<Suggestion>> {
        let app = self.get(id)?;
        let t = traits(&app.appimage_path)?;
        let profile = self.profile(id, DEFAULT_PROFILE)?;
        let sandboxed = self.sandbox_settings(id)?.enabled();
        let dismissed = |key: &str| -> Result<bool> {
            Ok(self
                .db
                .get_setting(id, &format!("dismissed:{key}"))?
                .is_some())
        };
        let has = |p: &str| profile.presets.iter().any(|x| x == p);
        let name = &app.name;
        let mut out = Vec::new();

        if t.needs_libfuse2
            && !host.libfuse2
            && !profile.extract_and_run
            && !sandboxed
            && !dismissed("unpack")?
        {
            out.push(Suggestion {
                key: "unpack".into(),
                title: "Start it without FUSE 2".into(),
                detail: format!(
                    "{name} uses an older AppImage runtime that needs libfuse.so.2, which is not installed here, so it cannot start as it is. Unpacking it instead of mounting it avoids that; it uses more disk space."
                ),
                action: "Unpack instead".into(),
                dismissible: true,
            });
        }

        let ozone_set = has("electron-wayland")
            || has("electron-auto")
            || has("x11")
            || profile
                .args
                .iter()
                .any(|a| a.starts_with("--ozone-platform"));
        if t.chromium && host.wayland && !ozone_set && !dismissed("electron-wayland")? {
            out.push(Suggestion {
                key: "electron-wayland".into(),
                title: "Let it use Wayland".into(),
                detail: format!(
                    "{name} is an Electron app. Older Electron versions always run through XWayland, which looks blurry on a scaled screen. The electron-auto preset lets it use Wayland when it can, and X11 otherwise; newer versions already do this, so it changes nothing for them."
                ),
                action: "Use Wayland".into(),
                dismissible: true,
            });
        }

        let chromium_sandbox_on = self.db.get_setting(id, CHROMIUM_SANDBOX)?.is_some();
        if chromium_sandbox_on {
            out.push(Suggestion {
                key: "chromium-sandbox-off".into(),
                title: "Chromium's own sandbox is on".into(),
                detail: format!(
                    "You turned it on for {name}. If the app no longer starts, or says the sandbox is not configured correctly, turn it off again."
                ),
                action: "Turn it off".into(),
                dismissible: false,
            });
        } else if t.chromium
            && t.own_no_sandbox
            && host.user_namespaces
            && !sandboxed
            && !has("no-sandbox")
            && !dismissed("chromium-sandbox")?
        {
            out.push(Suggestion {
                key: "chromium-sandbox".into(),
                title: "Turn Chromium's own sandbox back on".into(),
                detail: format!(
                    "{name} starts with --no-sandbox, which switches off the protection Chromium puts around web content. This computer lets apps use it without that flag. If the app then fails to start, turning it off again is one click."
                ),
                action: "Turn it on".into(),
                dismissible: true,
            });
        }

        let old = self.versions(id)?;
        let older: Vec<_> = old.iter().skip(1).collect();
        let size: u64 = older
            .iter()
            .filter_map(|v| fs::metadata(&v.path).ok())
            .map(|m| m.len())
            .sum();
        if !older.is_empty() && size >= OLD_VERSIONS_THRESHOLD && !dismissed("old-versions")? {
            out.push(Suggestion {
                key: "old-versions".into(),
                title: format!("Free up {} MB of old versions", mib(size)),
                detail: format!(
                    "{} previous versions of {name} are kept for rolling back. Keeping only the newest one frees {} MB.",
                    old.len(),
                    mib(size)
                ),
                action: "Keep only the newest".into(),
                dismissible: true,
            });
        }
        Ok(out)
    }

    /// Make the change a suggestion describes. Returns what was done, in words.
    pub fn apply_suggestion(&self, id: &str, key: &str) -> Result<String> {
        self.get(id)?;
        match key {
            "unpack" => {
                let mut p = self.profile(id, DEFAULT_PROFILE)?;
                p.extract_and_run = true;
                self.set_profile(id, DEFAULT_PROFILE, &p)?;
                Ok("It will be unpacked instead of mounted. Undo it on the Launch tab.".into())
            }
            "electron-wayland" => {
                let mut p = self.profile(id, DEFAULT_PROFILE)?;
                if !p.presets.iter().any(|x| x == "electron-auto") {
                    p.presets.push("electron-auto".into());
                }
                self.set_profile(id, DEFAULT_PROFILE, &p)?;
                Ok("Added the electron-auto preset. Undo it on the Launch tab.".into())
            }
            "chromium-sandbox" => {
                self.db.set_setting(id, CHROMIUM_SANDBOX, Some("on"))?;
                self.refresh_integration(id)?;
                Ok("Chromium's sandbox is on from the next start.".into())
            }
            "chromium-sandbox-off" => {
                self.db.set_setting(id, CHROMIUM_SANDBOX, None)?;
                // Asked once, answered: do not offer it again.
                self.db
                    .set_setting(id, "dismissed:chromium-sandbox", Some("1"))?;
                self.refresh_integration(id)?;
                Ok("The app starts with --no-sandbox again.".into())
            }
            "old-versions" => {
                self.prune_versions(id, 1)?;
                Ok("Kept only the newest previous version.".into())
            }
            other => Err(Error::Other(format!("unknown suggestion '{other}'"))),
        }
    }

    /// Stop offering a suggestion for this app.
    pub fn dismiss_suggestion(&self, id: &str, key: &str) -> Result<()> {
        self.get(id)?;
        self.db
            .set_setting(id, &format!("dismissed:{key}"), Some("1"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::{self, Item};
    use crate::library::AddOptions;
    use crate::paths::Dirs;

    fn electron_like(exec: &str, runtime_note: Option<&str>) -> Vec<u8> {
        let desktop =
            format!("[Desktop Entry]\nType=Application\nName=Chatty\nExec={exec}\nIcon=icon\n");
        fixture::build_appimage(
            runtime_note,
            vec![
                Item::File("/chatty.desktop", desktop.into_bytes()),
                Item::File("/icon.png", fixture::png(256)),
                Item::File("/AppRun", b"#!/bin/sh\nexit 0\n".to_vec()),
                Item::File("/chrome-sandbox", b"x".to_vec()),
                Item::File("/resources/app.asar", b"x".to_vec()),
            ],
        )
    }

    fn lib_with(image: Vec<u8>) -> (tempfile::TempDir, Library, String) {
        let tmp = tempfile::tempdir().unwrap();
        let lib =
            Library::open(Dirs::with_root(tmp.path()), tmp.path().join("appofholding")).unwrap();
        let p = tmp.path().join("Chatty.AppImage");
        fs::write(&p, image).unwrap();
        let id = lib.add(&p, &AddOptions::default()).unwrap().id;
        (tmp, lib, id)
    }

    const LINUX_DESKTOP: Host = Host {
        wayland: true,
        user_namespaces: true,
        libfuse2: true,
    };

    fn keys(v: &[Suggestion]) -> Vec<&str> {
        v.iter().map(|s| s.key.as_str()).collect()
    }

    #[test]
    fn an_electron_app_gets_wayland_and_sandbox_suggestions_that_apply_and_undo() {
        let (_t, lib, id) = lib_with(electron_like("AppRun --no-sandbox %U", None));
        let s = lib.suggestions(&id, &LINUX_DESKTOP).unwrap();
        assert_eq!(keys(&s), ["electron-wayland", "chromium-sandbox"]);

        // Wayland: adds the preset, after which it is not suggested again.
        lib.apply_suggestion(&id, "electron-wayland").unwrap();
        assert!(
            lib.profile(&id, DEFAULT_PROFILE)
                .unwrap()
                .presets
                .contains(&"electron-auto".to_string())
        );

        // Sandbox: the menu entry loses the app's own --no-sandbox ...
        let entry = || fs::read_to_string(lib.get(&id).unwrap().desktop_file).unwrap();
        assert!(entry().contains("--no-sandbox"));
        lib.apply_suggestion(&id, "chromium-sandbox").unwrap();
        assert!(!entry().contains("--no-sandbox"), "{}", entry());
        assert_eq!(
            keys(&lib.suggestions(&id, &LINUX_DESKTOP).unwrap()),
            ["chromium-sandbox-off"]
        );
        // ... and getting it back is one click, after which it is not offered again.
        lib.apply_suggestion(&id, "chromium-sandbox-off").unwrap();
        assert!(entry().contains("--no-sandbox"));
        assert!(lib.suggestions(&id, &LINUX_DESKTOP).unwrap().is_empty());
    }

    #[test]
    fn suggestions_respect_the_computer_and_the_apps_settings() {
        let (_t, lib, id) = lib_with(electron_like("AppRun --no-sandbox %U", None));
        // X11 session, no user namespaces (e.g. Ubuntu's AppArmor restriction): nothing to say.
        let limited = Host {
            wayland: false,
            user_namespaces: false,
            libfuse2: true,
        };
        assert!(lib.suggestions(&id, &limited).unwrap().is_empty());
        // Our own sandbox already handles Chromium; an app without --no-sandbox needs nothing.
        lib.set_sandbox(
            &id,
            &crate::sandbox::Settings::for_level(crate::sandbox::Level::Standard),
        )
        .unwrap();
        assert_eq!(
            keys(&lib.suggestions(&id, &LINUX_DESKTOP).unwrap()),
            ["electron-wayland"]
        );
        // Dismissed is dismissed.
        lib.dismiss_suggestion(&id, "electron-wayland").unwrap();
        assert!(lib.suggestions(&id, &LINUX_DESKTOP).unwrap().is_empty());

        let (_t2, lib2, id2) = lib_with(electron_like("AppRun %U", None));
        assert_eq!(
            keys(&lib2.suggestions(&id2, &LINUX_DESKTOP).unwrap()),
            ["electron-wayland"]
        );
        // A plain (non-Electron) app gets nothing.
        let tmp = tempfile::tempdir().unwrap();
        let lib3 = Library::open(Dirs::with_root(tmp.path()), tmp.path().join("x")).unwrap();
        let p = tmp.path().join("Foo.AppImage");
        fs::write(&p, fixture::simple_appimage("Foo App", None)).unwrap();
        let id3 = lib3.add(&p, &AddOptions::default()).unwrap().id;
        assert!(lib3.suggestions(&id3, &LINUX_DESKTOP).unwrap().is_empty());
    }

    #[test]
    fn an_old_runtime_without_libfuse2_is_unpacked_instead() {
        // The classic runtime names libfuse.so.2; the note lands in the runtime part of the file.
        let (_t, lib, id) = lib_with(electron_like("AppRun %U", Some("libfuse.so.2")));
        let no_fuse = Host {
            wayland: false,
            user_namespaces: true,
            libfuse2: false,
        };
        assert_eq!(keys(&lib.suggestions(&id, &no_fuse).unwrap()), ["unpack"]);
        lib.apply_suggestion(&id, "unpack").unwrap();
        assert!(lib.profile(&id, DEFAULT_PROFILE).unwrap().extract_and_run);
        assert!(lib.suggestions(&id, &no_fuse).unwrap().is_empty());
        // With libfuse2 installed it starts fine as it is.
        let (_t2, lib2, id2) = lib_with(electron_like("AppRun %U", Some("libfuse.so.2")));
        assert!(
            !keys(
                &lib2
                    .suggestions(
                        &id2,
                        &Host {
                            libfuse2: true,
                            ..no_fuse
                        }
                    )
                    .unwrap()
            )
            .contains(&"unpack")
        );
        assert!(lib.apply_suggestion(&id, "nonsense").is_err());
    }
}
