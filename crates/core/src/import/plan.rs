//! Planning an import: what to adopt, move or leave for each AppImage found.

use super::*;

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

pub(super) fn in_volatile_place(path: &Path, home: &Path) -> bool {
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

pub(super) fn unique_id(
    base: &str,
    existing: &[String],
    taken: &BTreeMap<String, PathBuf>,
) -> String {
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|c| !existing.contains(c) && !taken.contains_key(c))
        .unwrap_or_else(|| slugify(base))
}
