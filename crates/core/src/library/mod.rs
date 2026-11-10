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
mod integrate;
pub use integrate::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Install {
    /// Copy into the library directory, leaving the original in place.
    #[default]
    Copy,
    /// Move into the library directory.
    Move,
    /// Adopt the file where it is. It stays the user's file: removing the app keeps it.
    InPlace,
}

/// What `remove` does with the AppImage file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilePolicy {
    /// Delete files the library owns; keep files that were adopted in place.
    #[default]
    Auto,
    Keep,
    Delete,
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

pub struct Library {
    pub dirs: Dirs,
    /// Absolute path to the `appofholding` binary written into `Exec=` lines.
    pub exec_bin: PathBuf,
    pub(crate) db: Db,
    pub(crate) warnings: std::cell::RefCell<Vec<String>>,
}

/// The hicolor folder a PNG of `px` pixels belongs in. Icon themes only look in the sizes their
/// `index.theme` lists, so an icon filed under e.g. `2816x2816` is never found and the menu shows
/// none: use the largest standard size that is not bigger than the image (desktops scale it).
fn hicolor_size(px: u32) -> u32 {
    const SIZES: [u32; 13] = [16, 22, 24, 32, 36, 48, 64, 72, 96, 128, 192, 256, 512];
    SIZES
        .iter()
        .rev()
        .copied()
        .find(|s| *s <= px)
        .unwrap_or(SIZES[0])
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
        let dest = if opts.install == Install::InPlace {
            source.clone()
        } else {
            lib_dir.join(file_name)
        };
        let in_place = source == dest;
        // Adopted from somewhere else: the file stays the user's.
        let external = in_place && source.parent() != Some(lib_dir.as_path());
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
            external,
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
        external: bool,
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
                // Adopted where it is: nothing to copy (and `in_place` is true, so unreachable).
                Install::InPlace => {}
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
            external_file: external,
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
        let app_id = Path::new(&ins.desktop_path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned());
        // The user chose to let a Chromium/Electron app use its own sandbox again: drop the
        // `--no-sandbox` its menu entry carries.
        let drop: &[&str] = if self
            .db
            .get_setting(id, crate::suggest::CHROMIUM_SANDBOX)?
            .is_some()
        {
            &["--no-sandbox"]
        } else {
            &[]
        };
        let text = integrate_entry(
            &ins.entry,
            id,
            &self.exec_bin,
            icon_file.is_some(),
            &extra,
            app_id.as_deref(),
            drop,
        );
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
                let s = hicolor_size(icon.size.filter(|s| *s > 0).unwrap_or(256));
                base.join(format!("{s}x{s}/apps"))
                    .join(format!("{name}.png"))
            }
        }
    }

    /// Remove the app: desktop entry, icon, database row, and (unless `keep_file`) the AppImage.
    pub fn remove(&self, id: &str, keep_file: bool) -> Result<AppRecord> {
        self.remove_with(
            id,
            if keep_file {
                FilePolicy::Keep
            } else {
                FilePolicy::Auto
            },
        )
    }

    /// Remove the app. With `FilePolicy::Auto` the AppImage file is deleted only if the library
    /// owns it; a file that was adopted in place is the user's and stays.
    pub fn remove_with(&self, id: &str, files: FilePolicy) -> Result<AppRecord> {
        let rec = self.get(id)?;
        let mut targets = vec![rec.desktop_file.clone()];
        targets.extend(rec.icon_file.clone());
        let delete_file = match files {
            FilePolicy::Delete => true,
            FilePolicy::Keep => false,
            FilePolicy::Auto => !rec.external_file,
        };
        if delete_file {
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
mod tests;
