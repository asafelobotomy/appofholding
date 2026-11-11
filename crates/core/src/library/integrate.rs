//! Inspecting an AppImage and integrating its desktop entry.

use super::*;

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
pub(super) fn pick_desktop(img: &AppImage, file_stem: &str) -> Result<(String, DesktopEntry)> {
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
/// and `Icon` change (plus a few additive keys); actions, categories, MIME types and
/// everything else are preserved.
///
/// `app_id` is the file name (without `.desktop`) the entry had inside the AppImage. Ours is
/// named `appofholding-<id>.desktop`, so a window that announces the original name (Qt and GTK
/// apps on Wayland use it as their app id) would no longer be matched to this entry, and the
/// taskbar would show a generic icon. `StartupWMClass` restores the link when the app does not
/// declare one itself.
pub fn integrate_entry(
    entry: &DesktopEntry,
    id: &str,
    exec_bin: &Path,
    icon_installed: bool,
    profiles: &[String],
    app_id: Option<&str>,
    drop: &[&str],
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
        args.extend(
            rest.into_iter()
                .skip(1)
                .filter(|a| !drop.contains(&a.as_str())),
        );
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
    if e.main("StartupWMClass").is_none()
        && let Some(app_id) = app_id.filter(|a| !a.is_empty())
    {
        e.set(MAIN, "StartupWMClass", app_id);
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
