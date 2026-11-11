//! Downloading and installing a self-update.

use super::*;

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

/// Packages from earlier attempts are not needed once a newer one is on its way (an unfinished
/// download of this same file is kept, so it can resume).
pub(super) fn forget_other_downloads(work_dir: &Path, keep: &str) {
    let partial = format!(".{keep}.part");
    let Ok(rd) = fs::read_dir(work_dir) else {
        return;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name != keep && name != partial && e.path().is_file() {
            let _ = fs::remove_file(e.path());
        }
    }
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
            forget_other_downloads(work_dir, &asset.name);
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
