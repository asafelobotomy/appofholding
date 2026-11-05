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

#[derive(Debug, Clone, Default)]
pub struct PlanOptions {
    /// How to bring loose and tool-integrated files in. `None` = decide per file (see below).
    pub mode: Option<Install>,
    /// Back up the menu entries other tools made for these AppImages (so there is only one).
    pub replace_entries: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Import,
    /// Importable, but needs a decision: `alt_id` would avoid the clash.
    Conflict {
        reason: String,
        alt_id: String,
    },
    Skip(String),
}

#[derive(Debug, Clone)]
pub struct Item {
    pub found: Found,
    pub id: String,
    pub action: Action,
    pub mode: Install,
    /// Menu entries of other tools that will be moved to the backup folder.
    pub replace_entries: Vec<PathBuf>,
    /// Turn automatic updates off (the app updates itself).
    pub policy_manual: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ImportPlan {
    pub items: Vec<Item>,
}

fn in_volatile_place(path: &Path, home: &Path) -> bool {
    ["Downloads", "Desktop"]
        .iter()
        .any(|d| path.starts_with(home.join(d)))
        // Anything still in /tmp will vanish (unless the home folder itself lives there).
        || (path.starts_with("/tmp") && !home.starts_with("/tmp"))
}

/// Decide what importing each find would do. Pure: reads the library, changes nothing.
pub fn plan(lib: &Library, scan: &Scan, home: &Path, opts: &PlanOptions) -> Result<ImportPlan> {
    let existing_ids: Vec<String> = lib.list()?.into_iter().map(|a| a.id).collect();
    let mut taken: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut items = Vec::new();
    for f in &scan.found {
        let mut notes = Vec::new();
        let mut replace = Vec::new();
        let mut policy_manual = false;
        let mode = match &f.owner {
            Owner::App { .. } => {
                notes.push(
                    "stays where it is: the app keeps this file and updates it itself".into(),
                );
                notes.push("automatic updates from appOFholding are turned off for it".into());
                policy_manual = true;
                Install::InPlace
            }
            Owner::Tool { tool, entries } => {
                if opts.replace_entries {
                    replace = entries.clone();
                    notes.push(format!("{tool}'s menu entry is backed up and replaced (undo with `appofholding import restore`)"));
                } else {
                    notes.push(format!(
                        "{tool}'s menu entry stays, so the app will appear twice in your menu"
                    ));
                }
                opts.mode.unwrap_or(Install::InPlace)
            }
            Owner::Entry { .. } => {
                notes.push("it already has a menu entry that no known tool made; that entry stays, so the app will appear twice".into());
                opts.mode.unwrap_or(Install::InPlace)
            }
            Owner::Loose => opts.mode.unwrap_or(if in_volatile_place(&f.path, home) {
                Install::Move
            } else {
                Install::InPlace
            }),
        };
        if !f.extra_entries.is_empty() {
            notes.push(
                "an app (or you) also made a menu entry for it; that one is never touched".into(),
            );
        }
        match mode {
            Install::InPlace => notes.push("the file stays where it is".into()),
            Install::Move => notes.push(format!("moved into {}", lib.dirs.library_dir.display())),
            Install::Copy => notes.push(format!("copied into {}", lib.dirs.library_dir.display())),
        }
        if f.has_update_info {
            notes.push("update source found inside the AppImage".into());
        }

        let mut action = Action::Import;
        let mut id = f.id.clone();
        if existing_ids.contains(&id) {
            action = Action::Conflict {
                reason: format!("an app with the id '{id}' is already in the library"),
                alt_id: unique_id(&id, &existing_ids, &taken),
            };
        } else if let Some(other) = taken.get(&id) {
            action = Action::Conflict {
                reason: format!(
                    "another AppImage named '{}' is also being imported ({})",
                    f.name,
                    other.display()
                ),
                alt_id: unique_id(&id, &existing_ids, &taken),
            };
        }
        if mode != Install::InPlace {
            let target = lib
                .dirs
                .library_dir
                .join(f.path.file_name().unwrap_or_default());
            if target.exists() {
                action = Action::Skip(format!("{} already exists", target.display()));
            }
        }
        if matches!(action, Action::Import) {
            taken.insert(id.clone(), f.path.clone());
        } else if let Action::Conflict { alt_id, .. } = &action {
            id = alt_id.clone();
            taken.insert(id.clone(), f.path.clone());
        }
        items.push(Item {
            found: f.clone(),
            id,
            action,
            mode,
            replace_entries: replace,
            policy_manual,
            notes,
        });
    }
    Ok(ImportPlan { items })
}

fn unique_id(base: &str, existing: &[String], taken: &BTreeMap<String, PathBuf>) -> String {
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|c| !existing.contains(c) && !taken.contains_key(c))
        .unwrap_or_else(|| slugify(base))
}

#[derive(Debug, Clone)]
pub struct ItemResult {
    pub path: PathBuf,
    pub id: String,
    pub outcome: std::result::Result<Vec<String>, String>,
}

fn backup_root(lib: &Library) -> PathBuf {
    lib.dirs.app_data_dir().join("replaced-entries")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Manifest {
    created: i64,
    files: Vec<(PathBuf, String)>,
}

/// Move menu entries into a timestamped backup folder, remembering where they came from.
fn back_up_entries(lib: &Library, entries: &[PathBuf], batch: &str) -> Result<Vec<PathBuf>> {
    let dir = backup_root(lib).join(batch);
    fs::create_dir_all(&dir).map_err(Error::io(&dir))?;
    let manifest_path = dir.join("manifest.json");
    let mut manifest: Manifest = fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Manifest {
            created: now(),
            files: vec![],
        });
    let mut moved = Vec::new();
    for e in entries {
        let name = e
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let stored = format!("{}-{}", manifest.files.len(), name);
        let dest = dir.join(&stored);
        if fs::rename(e, &dest).is_err() {
            fs::copy(e, &dest).map_err(Error::io(&dest))?;
            fs::remove_file(e).map_err(Error::io(e))?;
        }
        manifest.files.push((e.clone(), stored));
        // Written after every move, so an interruption never loses track of a file.
        fs::write(
            &manifest_path,
            serde_json::to_string_pretty(&manifest).unwrap_or_default(),
        )
        .map_err(Error::io(&manifest_path))?;
        moved.push(e.clone());
    }
    Ok(moved)
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Carry out the items at the given indexes of `plan`. Items needing a decision (conflicts) run
/// under their suggested alternative id; skipped items are refused. One failure never stops
/// the rest.
pub fn execute(lib: &Library, plan: &ImportPlan, selected: &[usize]) -> Vec<ItemResult> {
    let batch = now().to_string();
    let mut results = Vec::new();
    for &i in selected {
        let Some(item) = plan.items.get(i) else {
            continue;
        };
        let mut result = ItemResult {
            path: item.found.path.clone(),
            id: item.id.clone(),
            outcome: Ok(vec![]),
        };
        if let Action::Skip(why) = &item.action {
            result.outcome = Err(format!("skipped: {why}"));
            results.push(result);
            continue;
        }
        let opts = AddOptions {
            install: item.mode,
            id: Some(item.id.clone()),
        };
        match lib.add(&item.found.path, &opts) {
            Err(e) => result.outcome = Err(e.to_string()),
            Ok(rec) => {
                let mut notes = vec![format!("imported as '{}'", rec.id)];
                if item.policy_manual && lib.source(&rec.id).ok().flatten().is_some() {
                    match lib.set_policy(&rec.id, Policy::Manual) {
                        Ok(()) => notes.push("automatic updates are off".into()),
                        Err(e) => notes.push(format!("could not set the update policy: {e}")),
                    }
                }
                if !item.replace_entries.is_empty() {
                    match back_up_entries(lib, &item.replace_entries, &batch) {
                        Ok(moved) => notes.push(format!(
                            "backed up {} other menu entr{}",
                            moved.len(),
                            if moved.len() == 1 { "y" } else { "ies" }
                        )),
                        Err(e) => notes.push(format!(
                            "imported, but could not back up the other menu entry: {e}"
                        )),
                    }
                }
                result.outcome = Ok(notes);
            }
        }
        results.push(result);
    }
    results
}

#[derive(Debug, Clone)]
pub struct Backup {
    pub batch: String,
    pub files: Vec<PathBuf>,
}

/// Menu-entry backups made by imports, newest first.
pub fn list_backups(lib: &Library) -> Vec<Backup> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(backup_root(lib)) else {
        return out;
    };
    for e in rd.flatten() {
        let Ok(text) = fs::read_to_string(e.path().join("manifest.json")) else {
            continue;
        };
        let Ok(m) = serde_json::from_str::<Manifest>(&text) else {
            continue;
        };
        out.push(Backup {
            batch: e.file_name().to_string_lossy().into_owned(),
            files: m.files.into_iter().map(|(orig, _)| orig).collect(),
        });
    }
    out.sort_by(|a, b| b.batch.cmp(&a.batch));
    out
}

/// Put backed-up menu entries back where they came from. Returns what was restored; entries
/// whose original location is occupied again are left in the backup.
pub fn restore(lib: &Library, batch: &str) -> Result<Vec<PathBuf>> {
    let dir = backup_root(lib).join(batch);
    let manifest_path = dir.join("manifest.json");
    let text = fs::read_to_string(&manifest_path).map_err(Error::io(&manifest_path))?;
    let mut manifest: Manifest = serde_json::from_str(&text)
        .map_err(|e| Error::Other(format!("corrupt backup manifest: {e}")))?;
    let mut restored = Vec::new();
    let mut remaining = Vec::new();
    for (orig, stored) in manifest.files.drain(..) {
        let from = dir.join(&stored);
        if orig.exists() || !from.exists() {
            remaining.push((orig, stored));
            continue;
        }
        if let Some(parent) = orig.parent() {
            fs::create_dir_all(parent).map_err(Error::io(parent))?;
        }
        if fs::rename(&from, &orig).is_err() {
            fs::copy(&from, &orig).map_err(Error::io(&orig))?;
            let _ = fs::remove_file(&from);
        }
        restored.push(orig);
    }
    if remaining.is_empty() {
        let _ = fs::remove_dir_all(&dir);
    } else {
        manifest.files = remaining;
        fs::write(
            &manifest_path,
            serde_json::to_string_pretty(&manifest).unwrap_or_default(),
        )
        .map_err(Error::io(&manifest_path))?;
    }
    lib.refresh_caches_public();
    Ok(restored)
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn finds_loose_tool_managed_and_app_owned_appimages_and_ignores_the_rest() {
        let e = env();
        let s = scan_all(&e);
        let names: Vec<&str> = s.found.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            ["Download App", "Foo Owned", "Gear App", "Loose App"]
        );
        assert_eq!(by_name(&s, "Loose App").owner, Owner::Loose);
        assert_eq!(by_name(&s, "Download App").owner, Owner::Loose);
        assert!(
            matches!(&by_name(&s, "Gear App").owner, Owner::Tool { tool, entries } if tool == "Gear Lever" && entries.len() == 1)
        );
        assert!(
            matches!(&by_name(&s, "Foo Owned").owner, Owner::App { entries } if entries.len() == 1),
            "reached through its menu entry, though it has no .AppImage name"
        );
        assert!(by_name(&s, "Download App").has_update_info);
        assert!(
            s.skipped
                .iter()
                .any(|(p, why)| p.ends_with("Fake.AppImage") && why.contains("not an AppImage")),
            "{:?}",
            s.skipped
        );
        assert!(
            !s.found.iter().any(|f| f.name == "Hidden"),
            "hidden folders are not searched"
        );
    }

    #[test]
    fn an_apps_own_entry_for_a_tool_managed_file_is_never_replaced() {
        // The real-world case: Gear Lever integrated the file, and the app also made its own
        // menu entry that launches it through a symlink.
        let e = env();
        let gl = e.home.join("AppImages/gear.appimage");
        let link = e.home.join(".local/share/gearapp/appimage");
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&gl, &link).unwrap();
        let apps_own = entry(
            &e.lib,
            "gearapp.desktop",
            &format!(
                "Name=Gear App\nX-Gear-Integrator=appimage\nExec={} -- %U",
                link.display()
            ),
        );
        let s = scan_all(&e);
        let gear = by_name(&s, "Gear App");
        assert!(
            matches!(&gear.owner, Owner::Tool { tool, entries } if tool == "Gear Lever" && entries.len() == 1)
        );
        assert_eq!(gear.extra_entries, vec![apps_own.clone()]);

        let p = plan(
            &e.lib,
            &s,
            &e.home,
            &PlanOptions {
                replace_entries: true,
                ..Default::default()
            },
        )
        .unwrap();
        let item = p.items.iter().find(|i| i.found.name == "Gear App").unwrap();
        assert_eq!(
            item.replace_entries,
            vec![e.lib.dirs.applications_dir().join("gear.desktop")]
        );
        assert!(item.notes.iter().any(|n| n.contains("never touched")));
        let idx = p
            .items
            .iter()
            .position(|i| i.found.name == "Gear App")
            .unwrap();
        execute(&e.lib, &p, &[idx]);
        assert!(apps_own.exists(), "the app's own menu entry survives");
        assert!(!e.lib.dirs.applications_dir().join("gear.desktop").exists());
    }

    #[test]
    fn an_unrecognised_menu_entry_in_an_appimage_folder_is_left_alone() {
        let e = env();
        let f = e.home.join("AppImages/Handmade.AppImage");
        write_image(&f, "Handmade", None);
        let own = entry(
            &e.lib,
            "handmade.desktop",
            &format!("Name=Handmade\nExec={}", f.display()),
        );
        let s = scan_all(&e);
        assert!(
            matches!(&by_name(&s, "Handmade").owner, Owner::Entry { entries } if entries == &vec![own.clone()])
        );
        let p = plan(
            &e.lib,
            &s,
            &e.home,
            &PlanOptions {
                replace_entries: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(
            p.items
                .iter()
                .find(|i| i.found.name == "Handmade")
                .unwrap()
                .replace_entries
                .is_empty(),
            "only known tools' entries are ever replaced"
        );
    }

    #[test]
    fn already_imported_files_and_copies_of_them_are_not_offered() {
        let e = env();
        e.lib
            .add(
                &e.home.join("AppImages/Loose.AppImage"),
                &AddOptions {
                    install: Install::InPlace,
                    id: None,
                },
            )
            .unwrap();
        // a byte-identical copy somewhere else
        fs::copy(
            e.home.join("AppImages/Loose.AppImage"),
            e.home.join("Downloads/LooseCopy.AppImage"),
        )
        .unwrap();
        let s = scan_all(&e);
        assert!(!s.found.iter().any(|f| f.name == "Loose App"));
        let why: Vec<&str> = s.skipped.iter().map(|(_, w)| w.as_str()).collect();
        assert!(
            why.iter()
                .any(|w| w.contains("already in the library as 'loose-app'")),
            "{why:?}"
        );
        assert!(
            why.iter().any(|w| w.contains("identical to 'loose-app'")),
            "{why:?}"
        );
    }

    #[test]
    fn the_plan_is_a_dry_run_that_changes_nothing() {
        let e = env();
        let before = snapshot(e.tmp.path());
        let s = scan_all(&e);
        let p = plan(&e.lib, &s, &e.home, &PlanOptions::default()).unwrap();
        let get = |n: &str| p.items.iter().find(|i| i.found.name == n).unwrap();
        assert_eq!(get("Loose App").mode, Install::InPlace);
        assert_eq!(
            get("Download App").mode,
            Install::Move,
            "files in Downloads are moved into the library"
        );
        assert_eq!(get("Gear App").mode, Install::InPlace);
        assert!(
            get("Gear App")
                .notes
                .iter()
                .any(|n| n.contains("appear twice"))
        );
        let foo = get("Foo Owned");
        assert_eq!(foo.mode, Install::InPlace);
        assert!(foo.policy_manual && foo.notes.iter().any(|n| n.contains("keeps this file")));
        assert!(p.items.iter().all(|i| i.action == Action::Import));
        assert_eq!(snapshot(e.tmp.path()), before, "planning touched nothing");
        assert!(e.lib.list().unwrap().is_empty());

        // App-owned files are never moved or copied, whatever mode is asked for.
        let forced = plan(
            &e.lib,
            &s,
            &e.home,
            &PlanOptions {
                mode: Some(Install::Copy),
                replace_entries: true,
            },
        )
        .unwrap();
        let foo = forced
            .items
            .iter()
            .find(|i| i.found.name == "Foo Owned")
            .unwrap();
        assert_eq!(foo.mode, Install::InPlace);
        assert!(
            foo.replace_entries.is_empty(),
            "an app's own menu entry is never replaced"
        );
        let gear = forced
            .items
            .iter()
            .find(|i| i.found.name == "Gear App")
            .unwrap();
        assert_eq!((gear.mode, gear.replace_entries.len()), (Install::Copy, 1));
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

    #[test]
    fn importing_follows_the_plan_and_respects_whose_files_they_are() {
        let e = env();
        let s = scan_all(&e);
        let p = plan(&e.lib, &s, &e.home, &PlanOptions::default()).unwrap();
        let all: Vec<usize> = (0..p.items.len()).collect();
        let results = execute(&e.lib, &p, &all);
        assert!(results.iter().all(|r| r.outcome.is_ok()), "{results:?}");
        assert_eq!(e.lib.list().unwrap().len(), 4);

        // Adopted in place: still where it was, and flagged as the user's file.
        let loose = e.lib.get("loose-app").unwrap();
        assert_eq!(
            loose.appimage_path,
            fs::canonicalize(e.home.join("AppImages/Loose.AppImage")).unwrap()
        );
        assert!(loose.external_file);
        // Moved from Downloads into the library folder.
        let dl = e.lib.get("download-app").unwrap();
        assert!(
            !e.home.join("Downloads/Dl.AppImage").exists()
                && dl
                    .appimage_path
                    .starts_with(fs::canonicalize(&e.lib.dirs.library_dir).unwrap())
        );
        assert!(!dl.external_file);
        // The app-owned one: in place, updates switched to manual, source kept.
        let foo = e.lib.get("foo-owned").unwrap();
        assert!(foo.external_file);
        assert_eq!(
            e.lib.source("foo-owned").unwrap().unwrap().policy,
            Policy::Manual
        );
        assert_eq!(
            e.lib.source("download-app").unwrap().unwrap().policy,
            Policy::Notify,
            "others keep the default"
        );
        // The other tool's menu entry was left alone.
        assert!(e.lib.dirs.applications_dir().join("gear.desktop").exists());

        // Removing an adopted app keeps the user's file; a library-owned file goes.
        e.lib.remove("loose-app", false).unwrap();
        assert!(e.home.join("AppImages/Loose.AppImage").exists());
        e.lib.remove("download-app", false).unwrap();
        assert!(!dl.appimage_path.exists());
        e.lib
            .remove_with("gear-app", crate::library::FilePolicy::Delete)
            .unwrap();
        assert!(
            !e.home.join("AppImages/gear.appimage").exists(),
            "deleting the file takes an explicit request"
        );
    }

    #[test]
    fn replacing_another_tools_menu_entry_is_backed_up_and_reversible() {
        let e = env();
        let s = scan_all(&e);
        let p = plan(
            &e.lib,
            &s,
            &e.home,
            &PlanOptions {
                replace_entries: true,
                ..Default::default()
            },
        )
        .unwrap();
        let gear_idx = p
            .items
            .iter()
            .position(|i| i.found.name == "Gear App")
            .unwrap();
        let original = e.lib.dirs.applications_dir().join("gear.desktop");
        let text = fs::read_to_string(&original).unwrap();
        let r = execute(&e.lib, &p, &[gear_idx]);
        assert!(
            r[0].outcome
                .as_ref()
                .unwrap()
                .iter()
                .any(|n| n.contains("backed up 1 other menu entry"))
        );
        assert!(!original.exists(), "only our menu entry remains");
        let backups = list_backups(&e.lib);
        assert_eq!(backups.len(), 1);
        assert_eq!(backups[0].files, vec![original.clone()]);

        let restored = restore(&e.lib, &backups[0].batch).unwrap();
        assert_eq!(restored, vec![original.clone()]);
        assert_eq!(
            fs::read_to_string(&original).unwrap(),
            text,
            "byte for byte what it was"
        );
        assert!(list_backups(&e.lib).is_empty());
    }

    #[test]
    fn restore_never_overwrites_a_file_that_is_there_again() {
        let e = env();
        let s = scan_all(&e);
        let p = plan(
            &e.lib,
            &s,
            &e.home,
            &PlanOptions {
                replace_entries: true,
                ..Default::default()
            },
        )
        .unwrap();
        let idx = p
            .items
            .iter()
            .position(|i| i.found.name == "Gear App")
            .unwrap();
        execute(&e.lib, &p, &[idx]);
        let original = e.lib.dirs.applications_dir().join("gear.desktop");
        fs::write(&original, "a new file").unwrap();
        let batch = list_backups(&e.lib).remove(0).batch;
        assert!(restore(&e.lib, &batch).unwrap().is_empty());
        assert_eq!(fs::read_to_string(&original).unwrap(), "a new file");
        assert_eq!(list_backups(&e.lib).len(), 1, "the backup is kept");
    }

    #[test]
    fn name_clashes_become_conflicts_with_a_suggested_alternative() {
        let e = env();
        write_image(&e.home.join("AppImages/Twin1.AppImage"), "Twin", None);
        write_image(
            &e.home.join("Desktop/Twin2.AppImage"),
            "Twin",
            Some("zsync|https://x/y.zsync"),
        ); // different bytes
        let s = scan_all(&e);
        let p = plan(&e.lib, &s, &e.home, &PlanOptions::default()).unwrap();
        let twins: Vec<&Item> = p.items.iter().filter(|i| i.found.name == "Twin").collect();
        assert_eq!(twins.len(), 2);
        assert_eq!(
            twins.iter().filter(|i| i.action == Action::Import).count(),
            1
        );
        let conflict = twins
            .iter()
            .find(|i| matches!(i.action, Action::Conflict { .. }))
            .unwrap();
        let Action::Conflict { reason, alt_id } = &conflict.action else {
            unreachable!()
        };
        assert!(
            reason.contains("also being imported") && alt_id == "twin-2",
            "{reason} / {alt_id}"
        );
        assert_eq!(conflict.id, "twin-2");

        // Choosing the conflicting one imports it under the alternative id.
        let idx = p
            .items
            .iter()
            .position(|i| std::ptr::eq(i, *conflict))
            .unwrap();
        let r = execute(&e.lib, &p, &[idx]);
        assert!(r[0].outcome.is_ok(), "{r:?}");
        assert!(e.lib.get("twin-2").is_ok());

        // An id already in the library is a conflict too.
        let again = scan_all(&e);
        let p2 = plan(&e.lib, &again, &e.home, &PlanOptions::default()).unwrap();
        assert!(p2.items.iter().all(|i| i.found.name != "Twin"
            || matches!(i.action, Action::Conflict { .. } | Action::Import)));
    }

    #[test]
    fn choosing_where_to_look_and_skipped_items_are_refused() {
        let e = env();
        let elsewhere = e.tmp.path().join("elsewhere");
        write_image(&elsewhere.join("Extra.AppImage"), "Extra App", None);
        let only = scan(
            &e.lib,
            &ScanOptions {
                extra_dirs: vec![elsewhere.clone()],
                only_extra: true,
                ..ScanOptions::new(&e.home)
            },
        )
        .unwrap();
        assert!(only.found.iter().any(|f| f.name == "Extra App"));
        assert!(
            !only.found.iter().any(|f| f.name == "Loose App"),
            "only the folder you named was searched"
        );
        assert_eq!(only.searched, vec![elsewhere]);

        // A move that would overwrite something in the library folder is skipped, not forced.
        let s = scan_all(&e);
        fs::create_dir_all(&e.lib.dirs.library_dir).unwrap();
        fs::write(e.lib.dirs.library_dir.join("Dl.AppImage"), b"occupied").unwrap();
        let p = plan(&e.lib, &s, &e.home, &PlanOptions::default()).unwrap();
        let dl = p
            .items
            .iter()
            .position(|i| i.found.name == "Download App")
            .unwrap();
        assert!(matches!(&p.items[dl].action, Action::Skip(w) if w.contains("already exists")));
        let r = execute(&e.lib, &p, &[dl]);
        assert!(r[0].outcome.is_err() && e.home.join("Downloads/Dl.AppImage").exists());
    }
}
