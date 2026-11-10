//! Importing AppImages that already exist on this machine.
//!
//! Three steps, so nothing changes until the user has seen the whole picture and said yes:
//!
//! 1. [`scan`] looks in the usual places for AppImage files, and follows the menu entries other
//!    tools and apps have written to find AppImages that live somewhere unusual.
//! 2. [`plan`] decides, for every find, what importing it would do. It changes nothing (a
//!    "dry run").
//! 3. [`execute`] carries out the chosen items.
//!
//! Who owns an AppImage matters:
//!
//! * **loose** files nobody refers to;
//! * files another **tool** (Gear Lever, AppImageLauncher, ...) has integrated;
//! * files an **app** keeps and updates itself (for example `~/.local/share/<app>/appimage`).
//!   These are never moved or copied, and automatic updates from appOFholding stay off.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::desktop::{self, DesktopEntry};
use crate::error::{Error, Result};
use crate::image;
use crate::library::{AddOptions, Install, Library, inspect};
use crate::paths::slugify;
use crate::source::Policy;
mod execute;
mod plan;
pub use execute::*;
pub use plan::*;

const MAX_DEPTH: usize = 2;
const MAX_FILES: usize = 2000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Owner {
    /// Nothing else refers to this file.
    Loose,
    /// Integrated into the menu by a known tool (Gear Lever, AppImageLauncher, ...). Only these
    /// entries can ever be replaced.
    Tool { tool: String, entries: Vec<PathBuf> },
    /// Has a menu entry of its own that no known tool recognises (made by hand, or by another
    /// program). Left alone.
    Entry { entries: Vec<PathBuf> },
    /// The app that this AppImage belongs to keeps it where it is and updates it itself.
    App { entries: Vec<PathBuf> },
}

#[derive(Debug, Clone)]
pub struct Found {
    pub path: PathBuf,
    pub size: u64,
    pub owner: Owner,
    pub name: String,
    pub id: String,
    pub version: Option<String>,
    pub has_update_info: bool,
    /// Menu entries that point at this file but are not the owner's (for example an app that
    /// also made its own entry for a Gear Lever managed file). Never touched.
    pub extra_entries: Vec<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct Scan {
    pub found: Vec<Found>,
    /// Files that look like AppImages but are not eligible, with the reason.
    pub skipped: Vec<(PathBuf, String)>,
    pub searched: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub home: PathBuf,
    /// Look here too (in addition to the usual places).
    pub extra_dirs: Vec<PathBuf>,
    /// Search only `extra_dirs`.
    pub only_extra: bool,
    /// Also read menu entries from these folders (the library's own folder is always read).
    pub extra_entry_dirs: Vec<PathBuf>,
}

impl ScanOptions {
    pub fn new(home: &Path) -> Self {
        Self {
            home: home.to_path_buf(),
            extra_dirs: vec![],
            only_extra: false,
            extra_entry_dirs: vec![],
        }
    }

    /// The places AppImages usually live.
    pub fn default_dirs(home: &Path) -> Vec<PathBuf> {
        [
            "Applications",
            "AppImages",
            "Downloads",
            "Desktop",
            "bin",
            ".local/bin",
            "opt",
            "Programs",
        ]
        .iter()
        .map(|d| home.join(d))
        .chain([PathBuf::from("/opt")])
        .collect()
    }
}

/// Cheap check before opening the whole image: an ELF file carrying the AppImage marker, or one
/// named like an AppImage.
fn looks_like_appimage(path: &Path) -> bool {
    let Ok(mut f) = fs::File::open(path) else {
        return false;
    };
    let mut head = [0u8; 11];
    if f.read_exact(&mut head).is_err() || &head[..4] != b"\x7fELF" {
        return false;
    }
    let marked = &head[8..10] == b"AI";
    let named = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("appimage"));
    marked || named
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if out.len() >= MAX_FILES {
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Ok(ft) = e.file_type() else { continue };
        let p = e.path();
        if ft.is_dir() {
            // Hidden folders hold caches, trash and updater leftovers, not apps the user keeps.
            if depth < MAX_DEPTH && !name.starts_with('.') {
                walk(&p, depth + 1, out);
            }
        } else if name.to_lowercase().ends_with(".appimage") && !name.starts_with('.') {
            out.push(p);
        }
    }
}

/// The AppImage a menu entry launches, if any: the first absolute path in `Exec`/`TryExec`
/// (skipping `env VAR=value` style wrappers) that is an AppImage.
fn entry_target(entry: &DesktopEntry) -> Option<PathBuf> {
    let from = |value: Option<&str>| -> Option<PathBuf> {
        let tokens = desktop::split_exec(value?);
        for t in tokens {
            if t == "env" || t.starts_with('-') || (t.contains('=') && !t.contains('/')) {
                continue;
            }
            if t.starts_with('/') {
                let p = PathBuf::from(&t);
                if p.is_file() && looks_like_appimage(&p) {
                    return Some(p);
                }
                // Only the program position counts; the arguments after it are not programs.
                return None;
            }
            return None;
        }
        None
    };
    from(entry.main("TryExec")).or_else(|| from(entry.main("Exec")))
}

struct EntryRef {
    file: PathBuf,
    tool: Option<&'static str>,
}

fn tool_of(file: &Path, entry: &DesktopEntry) -> Option<&'static str> {
    let stem = file
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let exec = entry.main("Exec").unwrap_or("");
    if stem.starts_with("appimagekit_") || entry.main("X-AppImage-Integrate").is_some() {
        Some("AppImageLauncher / appimaged")
    } else if exec.contains("DESKTOPINTEGRATION=1") {
        Some("Gear Lever")
    } else {
        None
    }
}

fn is_ours(file: &Path) -> bool {
    file.file_name()
        .is_some_and(|n| n.to_string_lossy().starts_with("appofholding-"))
}

/// Find importable AppImages. Reads, never writes.
pub fn scan(lib: &Library, opts: &ScanOptions) -> Result<Scan> {
    let mut scan = Scan::default();

    // 1. Files in the usual places.
    let mut dirs: Vec<PathBuf> = if opts.only_extra {
        vec![]
    } else {
        ScanOptions::default_dirs(&opts.home)
    };
    dirs.extend(opts.extra_dirs.iter().cloned());
    dirs.retain(|d| d.is_dir());
    dirs.dedup();
    let mut files: Vec<PathBuf> = Vec::new();
    for d in &dirs {
        walk(d, 0, &mut files);
    }
    scan.searched = dirs;
    let collection_dirs: Vec<PathBuf> = scan
        .searched
        .iter()
        .filter_map(|d| fs::canonicalize(d).ok())
        .collect();

    // 2. Files that menu entries launch (these find AppImages kept in unusual places).
    let mut entry_dirs = vec![lib.dirs.applications_dir()];
    entry_dirs.extend(opts.extra_entry_dirs.iter().cloned());
    let mut refs: BTreeMap<PathBuf, Vec<EntryRef>> = BTreeMap::new();
    for dir in &entry_dirs {
        let Ok(rd) = fs::read_dir(dir) else { continue };
        for e in rd.flatten() {
            let file = e.path();
            if file.extension().is_none_or(|x| x != "desktop") || is_ours(&file) {
                continue;
            }
            let Ok(text) = fs::read_to_string(&file) else {
                continue;
            };
            let Ok(entry) = DesktopEntry::parse(&text) else {
                continue;
            };
            if let Some(target) = entry_target(&entry) {
                let canon = fs::canonicalize(&target).unwrap_or(target);
                let tool = tool_of(&file, &entry);
                refs.entry(canon).or_default().push(EntryRef { file, tool });
            }
        }
    }
    for canon in refs.keys() {
        files.push(canon.clone());
    }

    // 3. Judge every distinct file.
    let existing: Vec<(String, u64, String)> = lib
        .list()?
        .into_iter()
        .map(|a| (a.id, a.size, a.sha256))
        .collect();
    let mut seen = std::collections::BTreeSet::new();
    for f in files {
        let canon = fs::canonicalize(&f).unwrap_or_else(|_| f.clone());
        if !seen.insert(canon.clone()) {
            continue;
        }
        let in_ours =
            canon.starts_with(lib.dirs.app_data_dir()) || canon.starts_with(&lib.dirs.cache_home);
        if in_ours {
            continue; // kept versions and unpacked copies
        }
        if !looks_like_appimage(&canon) {
            scan.skipped.push((canon, "not an AppImage".into()));
            continue;
        }
        if let Some(app) = lib.list()?.into_iter().find(|a| a.appimage_path == canon) {
            scan.skipped
                .push((canon, format!("already in the library as '{}'", app.id)));
            continue;
        }
        let size = fs::metadata(&canon).map(|m| m.len()).unwrap_or(0);
        if existing.iter().any(|(_, s, _)| *s == size) {
            let (sha, _) = image::hash_file(&canon)?;
            if let Some((id, _, _)) = existing.iter().find(|(_, s, h)| *s == size && *h == sha) {
                scan.skipped.push((
                    canon,
                    format!("identical to '{id}', which is already in the library"),
                ));
                continue;
            }
        }
        let ins = match inspect(&canon) {
            Ok(i) => i,
            Err(e) => {
                scan.skipped
                    .push((canon, format!("cannot be read as an AppImage ({e})")));
                continue;
            }
        };
        let (owner, extra_entries) = match refs.get(&canon) {
            None => (Owner::Loose, vec![]),
            Some(rs) => {
                let tool_entries: Vec<PathBuf> = rs
                    .iter()
                    .filter(|r| r.tool.is_some())
                    .map(|r| r.file.clone())
                    .collect();
                let other: Vec<PathBuf> = rs
                    .iter()
                    .filter(|r| r.tool.is_none())
                    .map(|r| r.file.clone())
                    .collect();
                if let Some(tool) = rs.iter().find_map(|r| r.tool) {
                    (
                        Owner::Tool {
                            tool: tool.to_string(),
                            entries: tool_entries,
                        },
                        other,
                    )
                } else if collection_dirs.iter().any(|d| canon.starts_with(d)) {
                    (Owner::Entry { entries: other }, vec![])
                } else {
                    (Owner::App { entries: other }, vec![])
                }
            }
        };
        scan.found.push(Found {
            id: ins.id.clone(),
            name: ins.name,
            version: ins.version,
            has_update_info: ins.image.update_info.is_some(),
            path: canon,
            size,
            owner,
            extra_entries,
        });
    }
    scan.found.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.path.cmp(&b.path))
    });
    Ok(scan)
}

#[cfg(test)]
mod tests;
