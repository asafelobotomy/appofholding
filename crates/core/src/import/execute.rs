//! Running an import plan, with backups of the menu entries it replaces.

use super::*;

#[derive(Debug, Clone)]
pub struct ItemResult {
    pub path: PathBuf,
    pub id: String,
    pub outcome: std::result::Result<Vec<String>, String>,
}

pub(super) fn backup_root(lib: &Library) -> PathBuf {
    lib.dirs.app_data_dir().join("replaced-entries")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Manifest {
    pub(super) created: i64,
    pub(super) files: Vec<(PathBuf, String)>,
}

/// Move menu entries into a timestamped backup folder, remembering where they came from.
pub(super) fn back_up_entries(
    lib: &Library,
    entries: &[PathBuf],
    batch: &str,
) -> Result<Vec<PathBuf>> {
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

pub(super) fn now() -> i64 {
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
