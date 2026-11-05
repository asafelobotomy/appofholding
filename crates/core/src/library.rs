//! The library: adding, listing and removing AppImages, including desktop integration.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::db::{AppRecord, Db};
use crate::desktop::{self, DesktopEntry, MAIN};
use crate::error::{Error, Result};
use crate::icon::{self, Icon, IconFormat};
use crate::image::{self, AppImage};
use crate::paths::{Dirs, desktop_id, slugify};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Install {
    /// Copy into the library directory, leaving the original in place.
    #[default]
    Copy,
    /// Move into the library directory.
    Move,
}

#[derive(Debug, Clone, Default)]
pub struct AddOptions {
    pub install: Install,
    /// Override the id derived from the app name.
    pub id: Option<String>,
}

/// What we learned from an AppImage without changing anything.
pub struct Inspection {
    pub image: AppImage,
    pub desktop_path: String,
    pub entry: DesktopEntry,
    pub name: String,
    pub id: String,
    pub version: Option<String>,
    pub icon: Option<Icon>,
}

pub fn inspect(path: &Path) -> Result<Inspection> {
    let image = AppImage::open(path)?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (desktop_path, entry) = pick_desktop(&image, &stem)?;
    let name = entry
        .main("Name")
        .map(str::to_string)
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| stem.clone());
    let version = entry
        .main("X-AppImage-Version")
        .map(str::to_string)
        .filter(|v| !v.is_empty());
    let icon = icon::find_icon(&image, entry.main("Icon"))?;
    let id = slugify(&name);
    Ok(Inspection {
        image,
        desktop_path,
        entry,
        name,
        id,
        version,
        icon,
    })
}

/// Choose the right `.desktop` file when an AppImage ships several (Gear Lever #410).
/// Prefers visible applications, then the one whose name resembles the file name.
fn pick_desktop(img: &AppImage, file_stem: &str) -> Result<(String, DesktopEntry)> {
    let mut paths: Vec<String> = img
        .list_dir("/")
        .into_iter()
        .filter(|(n, dir)| !dir && n.ends_with(".desktop"))
        .map(|(n, _)| format!("/{n}"))
        .collect();
    if paths.is_empty() {
        paths = img
            .list_dir("/usr/share/applications")
            .into_iter()
            .filter(|(n, dir)| !dir && n.ends_with(".desktop"))
            .map(|(n, _)| format!("/usr/share/applications/{n}"))
            .collect();
    }
    let stem = slugify(file_stem);
    let mut best: Option<(i32, String, DesktopEntry)> = None;
    for p in paths {
        let Some(bytes) = img.read_file(&p)? else {
            continue;
        };
        let Ok(entry) = DesktopEntry::parse(&String::from_utf8_lossy(&bytes)) else {
            continue;
        };
        let mut score = 0;
        if entry.is_visible_application() {
            score += 10;
        }
        let name = slugify(entry.main("Name").unwrap_or(""));
        let fname = slugify(
            p.rsplit('/')
                .next()
                .unwrap_or("")
                .trim_end_matches(".desktop"),
        );
        for s in [&name, &fname] {
            if !s.is_empty()
                && s != "app"
                && (stem.contains(s.as_str()) || s.contains(stem.as_str()))
            {
                score += 5;
            }
        }
        if best.as_ref().is_none_or(|(b, _, _)| score > *b) {
            best = Some((score, p, entry));
        }
    }
    best.map(|(_, p, e)| (p, e)).ok_or(Error::NoDesktopEntry)
}

/// Rewrite an AppImage's own desktop entry into the one we install. Only `Exec`, `TryExec`
/// and `Icon` change (plus two additive keys); actions, categories, MIME types and
/// everything else are preserved.
pub fn integrate_entry(
    entry: &DesktopEntry,
    id: &str,
    exec_bin: &Path,
    icon_installed: bool,
    profiles: &[String],
) -> String {
    let mut e = entry.clone();
    let wrap = |original: Option<&str>| -> String {
        let rest = original.map(desktop::split_exec).unwrap_or_default();
        let mut args = vec![
            exec_bin.to_string_lossy().into_owned(),
            "run".into(),
            id.to_string(),
        ];
        // The first token is the AppImage's own binary (AppRun etc.); our launcher replaces it.
        args.extend(rest.into_iter().skip(1));
        desktop::join_exec(&args)
    };
    let main_exec = wrap(entry.main("Exec"));
    e.set(MAIN, "Exec", &main_exec);
    e.remove(MAIN, "TryExec");
    if icon_installed {
        e.set(MAIN, "Icon", &desktop_id(id));
    }
    if e.main("StartupNotify").is_none() {
        e.set(MAIN, "StartupNotify", "true");
    }
    e.set(MAIN, "X-AppOfHolding-Id", id);
    for g in e.action_groups() {
        let new = wrap(entry.get(&g, "Exec"));
        e.set(&g, "Exec", &new);
    }
    // Each extra launch profile becomes an entry in the app's right-click menu.
    if !profiles.is_empty() {
        let mut actions = entry.main("Actions").unwrap_or("").to_string();
        for name in profiles {
            let key = format!("appofholding-{name}");
            let group = format!("Desktop Action {key}");
            if !actions.is_empty() && !actions.ends_with(';') {
                actions.push(';');
            }
            actions.push_str(&format!("{key};"));
            e.add_group(&group);
            e.set(&group, "Name", &format!("Launch: {name}"));
            let exec = desktop::join_exec(&[
                exec_bin.to_string_lossy().into_owned(),
                "run".into(),
                "--profile".into(),
                name.clone(),
                id.to_string(),
                "%U".into(),
            ]);
            e.set(&group, "Exec", &exec);
        }
        e.set(MAIN, "Actions", &actions);
    }
    e.render()
}

pub struct Library {
    pub dirs: Dirs,
    /// Absolute path to the `appofholding` binary written into `Exec=` lines.
    pub exec_bin: PathBuf,
    pub(crate) db: Db,
    pub(crate) warnings: std::cell::RefCell<Vec<String>>,
}

impl Library {
    pub fn open(dirs: Dirs, exec_bin: PathBuf) -> Result<Self> {
        let db = Db::open(&dirs.db_path())?;
        Ok(Self {
            dirs,
            exec_bin,
            db,
            warnings: Default::default(),
        })
    }

    pub fn list(&self) -> Result<Vec<AppRecord>> {
        self.db.list()
    }

    /// Delete an app's private sandbox home (its settings, logins, downloads...). Never done
    /// implicitly by `remove`, because it cannot be undone.
    pub fn purge_private_home(&self, id: &str) -> Result<bool> {
        let home = self.dirs.private_home(id);
        match fs::remove_dir_all(&home) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(Error::io(&home)(e)),
        }
    }

    pub fn get(&self, id: &str) -> Result<AppRecord> {
        self.db
            .get(id)?
            .ok_or_else(|| Error::NotFound(id.to_string()))
    }

    pub fn add(&self, source: &Path, opts: &AddOptions) -> Result<AppRecord> {
        let source = fs::canonicalize(source).map_err(Error::io(source))?;
        let ins = inspect(&source)?;
        let id = opts
            .id
            .clone()
            .map(|i| slugify(&i))
            .unwrap_or_else(|| ins.id.clone());
        if self.db.get(&id)?.is_some() {
            return Err(Error::AlreadyAdded(id));
        }

        fs::create_dir_all(&self.dirs.library_dir).map_err(Error::io(&self.dirs.library_dir))?;
        let lib_dir =
            fs::canonicalize(&self.dirs.library_dir).map_err(Error::io(&self.dirs.library_dir))?;
        let file_name = source
            .file_name()
            .ok_or_else(|| Error::Other("source has no file name".into()))?;
        let dest = lib_dir.join(file_name);
        let in_place = source == dest;
        if self.db.find_by_path(&dest)?.is_some() {
            return Err(Error::AlreadyAdded(dest.display().to_string()));
        }
        if !in_place && dest.exists() {
            return Err(Error::Other(format!("{} already exists", dest.display())));
        }

        let (sha256, size) = image::hash_file(&source)?;
        let mut created: Vec<PathBuf> = Vec::new();
        let result = self.add_inner(
            &ins,
            &id,
            &source,
            &dest,
            in_place,
            opts.install,
            sha256,
            size,
            &mut created,
        );
        if result.is_err() {
            for p in created.iter().rev() {
                let _ = fs::remove_file(p);
            }
            if !in_place && opts.install == Install::Move && !source.exists() && dest.exists() {
                let _ = fs::rename(&dest, &source);
            }
        } else {
            self.refresh_caches();
        }
        // Remember where the AppImage says it wants to be updated from.
        if let Ok(rec) = &result
            && let Some(Ok(p)) = rec.update_info.as_deref().map(crate::source::parse_input)
        {
            let _ = self.db.upsert_source(&rec.id, &p.source);
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn add_inner(
        &self,
        ins: &Inspection,
        id: &str,
        source: &Path,
        dest: &Path,
        in_place: bool,
        install: Install,
        sha256: String,
        size: u64,
        created: &mut Vec<PathBuf>,
    ) -> Result<AppRecord> {
        if !in_place {
            match install {
                Install::Copy => {
                    fs::copy(source, dest).map_err(Error::io(dest))?;
                    created.push(dest.to_path_buf());
                }
                Install::Move => {
                    if fs::rename(source, dest).is_err() {
                        // Cross-device move.
                        fs::copy(source, dest).map_err(Error::io(dest))?;
                        created.push(dest.to_path_buf());
                        fs::remove_file(source).map_err(Error::io(source))?;
                    }
                }
            }
        }
        let mut perms = fs::metadata(dest).map_err(Error::io(dest))?.permissions();
        perms.set_mode(perms.mode() | 0o755);
        fs::set_permissions(dest, perms).map_err(Error::io(dest))?;

        let (desktop_file, icon_file) = self.write_integration(ins, id)?;
        created.extend(icon_file.clone());
        created.push(desktop_file.clone());

        let rec = AppRecord {
            id: id.to_string(),
            name: ins.name.clone(),
            appimage_path: dest.to_path_buf(),
            desktop_file,
            icon_file,
            version: ins.version.clone(),
            sha256,
            size,
            appimage_type: ins.image.kind,
            update_info: ins.image.update_info.clone(),
            added_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() as i64),
        };
        self.db.insert(&rec)?;
        Ok(rec)
    }

    /// (Re)write the menu entry and icon for `id` from an inspected AppImage. Returns
    /// (desktop file, icon file). Only generated files are touched; user settings live in the
    /// database, so this is safe to call again after an update.
    pub(crate) fn write_integration(
        &self,
        ins: &Inspection,
        id: &str,
    ) -> Result<(PathBuf, Option<PathBuf>)> {
        let icon_file = match &ins.icon {
            Some(icon) => {
                let p = self.icon_path(id, icon);
                write_atomic(&p, &icon.data)?;
                Some(p)
            }
            None => None,
        };
        let extra: Vec<String> = self
            .db
            .list_profiles(id)?
            .into_iter()
            .map(|(n, _)| n)
            .filter(|n| n != crate::launch::DEFAULT_PROFILE)
            .collect();
        let text = integrate_entry(&ins.entry, id, &self.exec_bin, icon_file.is_some(), &extra);
        let desktop_file = self.dirs.desktop_file(id);
        write_atomic(&desktop_file, text.as_bytes())?;
        Ok((desktop_file, icon_file))
    }

    fn icon_path(&self, id: &str, icon: &Icon) -> PathBuf {
        let base = self.dirs.icons_dir().join("hicolor");
        let name = desktop_id(id);
        match icon.format {
            IconFormat::Svg => base.join("scalable/apps").join(format!("{name}.svg")),
            IconFormat::Png => {
                let s = icon.size.filter(|s| *s > 0).unwrap_or(256);
                base.join(format!("{s}x{s}/apps"))
                    .join(format!("{name}.png"))
            }
        }
    }

    /// Remove the app: desktop entry, icon, database row, and (unless `keep_file`) the AppImage.
    pub fn remove(&self, id: &str, keep_file: bool) -> Result<AppRecord> {
        let rec = self.get(id)?;
        let mut targets = vec![rec.desktop_file.clone()];
        targets.extend(rec.icon_file.clone());
        if !keep_file {
            targets.push(rec.appimage_path.clone());
        }
        for t in targets {
            match fs::remove_file(&t) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(Error::io(&t)(e)),
            }
        }
        self.db.delete(id)?;
        let _ = self.gc_extracted(); // its unpacked copies are no longer needed
        self.refresh_caches();
        Ok(rec)
    }

    pub(crate) fn refresh_caches_public(&self) {
        self.refresh_caches();
    }

    fn refresh_caches(&self) {
        if self.dirs.refresh_caches {
            // Best effort: desktop environments also watch the directory.
            let _ = Command::new("update-desktop-database")
                .arg(self.dirs.applications_dir())
                .status();
        }
    }
}

pub(crate) fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Other("path has no parent".into()))?;
    fs::create_dir_all(parent).map_err(Error::io(parent))?;
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    fs::write(&tmp, data).map_err(Error::io(&tmp))?;
    fs::rename(&tmp, path).map_err(Error::io(path))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::{self, Item};

    struct Env {
        _tmp: tempfile::TempDir,
        root: PathBuf,
        lib: Library,
    }

    fn env() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        // A path with a space exercises Exec quoting.
        let root = tmp.path().join("my root");
        fs::create_dir_all(&root).unwrap();
        let lib = Library::open(Dirs::with_root(&root), root.join("bin/appofholding")).unwrap();
        Env {
            _tmp: tmp,
            root,
            lib,
        }
    }

    fn write_image(env: &Env, file: &str, bytes: Vec<u8>) -> PathBuf {
        let dir = env.root.join("downloads");
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join(file);
        fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn inspect_reads_metadata_without_running_anything() {
        let e = env();
        let p = write_image(
            &e,
            "Foo-1.2.3.AppImage",
            fixture::simple_appimage("Foo App", Some("gh-releases-zsync|o|r|latest|*.zsync")),
        );
        let i = inspect(&p).unwrap();
        assert_eq!(i.name, "Foo App");
        assert_eq!(i.id, "foo-app");
        assert_eq!(i.version.as_deref(), Some("1.2.3"));
        assert_eq!(
            i.image.update_info.as_deref(),
            Some("gh-releases-zsync|o|r|latest|*.zsync")
        );
        assert_eq!(i.icon.unwrap().size, Some(256));
    }

    #[test]
    fn add_integrates_and_preserves_desktop_content() {
        let e = env();
        let p = write_image(
            &e,
            "Foo.AppImage",
            fixture::simple_appimage("Foo App", None),
        );
        let rec = e.lib.add(&p, &AddOptions::default()).unwrap();

        assert_eq!(rec.id, "foo-app");
        assert!(p.exists(), "copy leaves the original");
        assert!(rec.appimage_path.exists());
        assert_eq!(
            rec.appimage_path.parent().unwrap(),
            fs::canonicalize(&e.lib.dirs.library_dir).unwrap()
        );
        assert_ne!(
            fs::metadata(&rec.appimage_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o111,
            0
        );
        assert!(
            rec.icon_file
                .as_ref()
                .unwrap()
                .ends_with("hicolor/256x256/apps/appofholding-foo-app.png")
        );
        assert!(rec.icon_file.as_ref().unwrap().exists());

        let text = fs::read_to_string(&rec.desktop_file).unwrap();
        let d = DesktopEntry::parse(&text).unwrap();
        let exec = desktop::split_exec(d.main("Exec").unwrap());
        assert_eq!(exec[0], e.lib.exec_bin.to_string_lossy());
        assert_eq!(&exec[1..], ["run", "foo-app", "--flag", "%U"]);
        assert_eq!(d.main("Icon"), Some("appofholding-foo-app"));
        assert_eq!(d.main("Categories"), Some("Utility;"));
        assert_eq!(d.main("X-AppOfHolding-Id"), Some("foo-app"));
        assert_eq!(d.main("StartupNotify"), Some("true"));
        // Desktop actions survive, rewired through our launcher.
        let action = desktop::split_exec(d.get("Desktop Action new", "Exec").unwrap());
        assert_eq!(&action[1..], ["run", "foo-app", "--new-window"]);
        assert_eq!(d.get("Desktop Action new", "Name"), Some("New window"));

        assert_eq!(e.lib.list().unwrap().len(), 1);
        assert_eq!(e.lib.get("foo-app").unwrap(), rec);
    }

    #[test]
    fn adding_twice_is_rejected_and_leaves_no_debris() {
        let e = env();
        let p = write_image(
            &e,
            "Foo.AppImage",
            fixture::simple_appimage("Foo App", None),
        );
        e.lib.add(&p, &AddOptions::default()).unwrap();
        assert!(matches!(
            e.lib.add(&p, &AddOptions::default()),
            Err(Error::AlreadyAdded(_))
        ));
        assert_eq!(e.lib.list().unwrap().len(), 1);
    }

    #[test]
    fn remove_cleans_everything_up() {
        let e = env();
        let p = write_image(
            &e,
            "Foo.AppImage",
            fixture::simple_appimage("Foo App", None),
        );
        let rec = e.lib.add(&p, &AddOptions::default()).unwrap();
        e.lib.remove("foo-app", false).unwrap();
        assert!(!rec.appimage_path.exists());
        assert!(!rec.desktop_file.exists());
        assert!(!rec.icon_file.unwrap().exists());
        assert!(e.lib.list().unwrap().is_empty());
        assert!(matches!(
            e.lib.remove("foo-app", false),
            Err(Error::NotFound(_))
        ));
    }

    #[test]
    fn remove_can_keep_the_appimage() {
        let e = env();
        let p = write_image(
            &e,
            "Foo.AppImage",
            fixture::simple_appimage("Foo App", None),
        );
        let rec = e.lib.add(&p, &AddOptions::default()).unwrap();
        e.lib.remove("foo-app", true).unwrap();
        assert!(rec.appimage_path.exists());
        assert!(!rec.desktop_file.exists());
    }

    #[test]
    fn move_mode_removes_the_original() {
        let e = env();
        let p = write_image(
            &e,
            "Foo.AppImage",
            fixture::simple_appimage("Foo App", None),
        );
        let rec = e
            .lib
            .add(
                &p,
                &AddOptions {
                    install: Install::Move,
                    id: None,
                },
            )
            .unwrap();
        assert!(!p.exists());
        assert!(rec.appimage_path.exists());
    }

    #[test]
    fn custom_id_and_in_place_add() {
        let e = env();
        fs::create_dir_all(&e.lib.dirs.library_dir).unwrap();
        let p = e.lib.dirs.library_dir.join("Already.AppImage");
        fs::write(&p, fixture::simple_appimage("Foo App", None)).unwrap();
        let rec = e
            .lib
            .add(
                &p,
                &AddOptions {
                    install: Install::Copy,
                    id: Some("My Foo".into()),
                },
            )
            .unwrap();
        assert_eq!(rec.id, "my-foo");
        assert_eq!(rec.appimage_path, fs::canonicalize(&p).unwrap());
    }

    #[test]
    fn picks_the_right_desktop_file_when_several_exist() {
        // digiKam ships showfoto and digikam entries; we must pick the one matching the file.
        let e = env();
        let showfoto = "[Desktop Entry]\nType=Application\nName=Showfoto\nExec=showfoto %F\n";
        let hidden =
            "[Desktop Entry]\nType=Application\nName=Helper\nNoDisplay=true\nExec=helper\n";
        let digikam =
            "[Desktop Entry]\nType=Application\nName=digiKam\nExec=digikam %U\nIcon=digikam\n";
        let bytes = fixture::build_appimage(
            None,
            vec![
                Item::File("/org.kde.showfoto.desktop", showfoto.into()),
                Item::File("/a-helper.desktop", hidden.into()),
                Item::File("/org.kde.digikam.desktop", digikam.into()),
                Item::File("/digikam.png", fixture::png(128)),
            ],
        );
        let p = write_image(&e, "digikam-8.4.0-x86-64.appimage", bytes);
        let i = inspect(&p).unwrap();
        assert_eq!(i.name, "digiKam");
        assert_eq!(i.desktop_path, "/org.kde.digikam.desktop");
        assert_eq!(i.icon.unwrap().size, Some(128));
    }

    #[test]
    fn icon_falls_back_to_hicolor_largest() {
        let e = env();
        let desktop = "[Desktop Entry]\nType=Application\nName=Bar\nExec=bar\nIcon=bar\n";
        let bytes = fixture::build_appimage(
            None,
            vec![
                Item::File("/bar.desktop", desktop.into()),
                Item::File(
                    "/usr/share/icons/hicolor/48x48/apps/bar.png",
                    fixture::png(48),
                ),
                Item::File(
                    "/usr/share/icons/hicolor/512x512/apps/bar.png",
                    fixture::png(512),
                ),
            ],
        );
        let i = inspect(&write_image(&e, "bar.AppImage", bytes)).unwrap();
        assert_eq!(i.icon.unwrap().size, Some(512));
    }

    #[test]
    fn rejects_non_appimages() {
        let e = env();
        let p = write_image(&e, "text.AppImage", b"hello world, not an elf".to_vec());
        assert!(matches!(inspect(&p), Err(Error::NotAppImage(_))));
        let mut elf_only = fixture::fake_runtime(None);
        elf_only.extend([0u8; 64]);
        let p = write_image(&e, "elf.AppImage", elf_only);
        assert!(matches!(inspect(&p), Err(Error::NotAppImage(_))));
    }

    #[test]
    fn rejects_appimage_without_desktop_entry() {
        let e = env();
        let bytes = fixture::build_appimage(None, vec![Item::File("/AppRun", b"x".to_vec())]);
        let p = write_image(&e, "nodesk.AppImage", bytes);
        assert!(matches!(inspect(&p), Err(Error::NoDesktopEntry)));
    }

    #[test]
    fn failed_add_rolls_back_copied_file() {
        let e = env();
        let bytes = fixture::build_appimage(None, vec![Item::File("/AppRun", b"x".to_vec())]);
        let p = write_image(&e, "nodesk.AppImage", bytes);
        assert!(e.lib.add(&p, &AddOptions::default()).is_err());
        let lib_dir = &e.lib.dirs.library_dir;
        assert!(!lib_dir.exists() || fs::read_dir(lib_dir).unwrap().next().is_none());
    }
}
