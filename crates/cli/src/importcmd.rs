//! `appofholding import`: find AppImages already on disk and bring them into the library.

use super::*;

/// What the user answered at the confirmation prompt.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Answer {
    No,
    /// Everything that is currently selected.
    Selected,
    /// Exactly these plan numbers (1-based).
    Numbers(Vec<usize>),
}

pub(crate) fn parse_answer(text: &str, count: usize) -> Answer {
    let t = text.trim().to_lowercase();
    if t == "y" || t == "yes" {
        return Answer::Selected;
    }
    if t.is_empty() || t == "n" || t == "no" {
        return Answer::No;
    }
    let nums: Option<Vec<usize>> = t
        .split([',', ' '])
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<usize>().ok().filter(|n| (1..=count).contains(n)))
        .collect();
    match nums {
        Some(v) if !v.is_empty() => Answer::Numbers(v),
        _ => Answer::No,
    }
}

pub(crate) fn short(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

pub(crate) fn import_cmd(args: ImportArgs) -> Result<()> {
    let lib = library()?;
    let home = PathBuf::from(std::env::var_os("HOME").context("$HOME is not set")?);
    if let Some(ImportAction::Restore { batch, list }) = args.action {
        let backups = import::list_backups(&lib);
        if backups.is_empty() {
            println!("No menu entries are backed up.");
            return Ok(());
        }
        if list {
            for b in &backups {
                println!(
                    "{}: {}",
                    b.batch,
                    b.files
                        .iter()
                        .map(|f| short(f, &home))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            return Ok(());
        }
        let batch = batch.unwrap_or_else(|| backups[0].batch.clone());
        let restored = import::restore(&lib, &batch)?;
        for r in &restored {
            println!("Restored {}", short(r, &home));
        }
        if restored.is_empty() {
            println!(
                "Nothing restored (the original files are in the way, or the backup is gone)."
            );
        }
        return Ok(());
    }

    let mode = match args.mode.as_deref() {
        None => None,
        Some("in-place" | "inplace") => Some(Install::InPlace),
        Some("copy") => Some(Install::Copy),
        Some("move") => Some(Install::Move),
        Some(other) => bail!("--mode must be in-place, copy or move (not '{other}')"),
    };
    let scan_opts = import::ScanOptions {
        extra_dirs: args.dirs.clone(),
        only_extra: args.only_dirs,
        ..import::ScanOptions::new(&home)
    };
    println!("Looking for AppImages...");
    let scan = import::scan(&lib, &scan_opts)?;
    let plan = import::plan(
        &lib,
        &scan,
        &home,
        &import::PlanOptions {
            mode,
            replace_entries: args.replace_entries,
        },
    )?;
    println!(
        "Searched: {}",
        scan.searched
            .iter()
            .map(|d| short(d, &home))
            .collect::<Vec<_>>()
            .join(", ")
    );

    // Which items are selected by default / by the filters.
    let matches = |item: &import::Item, n: usize, list: &[String]| {
        list.iter().any(|w| {
            let w = w.trim().to_lowercase();
            w == item.id
                || w == item.found.name.to_lowercase()
                || w.parse::<usize>().ok() == Some(n)
        })
    };
    let mut selected: Vec<usize> = Vec::new();
    for (i, item) in plan.items.iter().enumerate() {
        let n = i + 1;
        let base = match &item.action {
            import::Action::Import | import::Action::Update { .. } => true,
            import::Action::Conflict { .. } => args.include_conflicts,
            import::Action::Skip(_) => false,
        };
        let wanted = if args.only.is_empty() {
            base
        } else {
            matches(item, n, &args.only)
        };
        if wanted
            && !matches(item, n, &args.exclude)
            && !matches!(item.action, import::Action::Skip(_))
        {
            selected.push(i);
        }
    }

    println!();
    if plan.items.is_empty() {
        println!("No importable AppImages found.");
    } else {
        println!("Found {} AppImage(s):\n", plan.items.len());
    }
    for (i, item) in plan.items.iter().enumerate() {
        let f = &item.found;
        let mark = if selected.contains(&i) { "[x]" } else { "[ ]" };
        let owner = match &f.owner {
            import::Owner::Loose => "loose file".to_string(),
            import::Owner::Tool { tool, .. } => format!("integrated by {tool}"),
            import::Owner::Entry { .. } => "has its own menu entry".to_string(),
            import::Owner::App { .. } => "kept by the app itself".to_string(),
        };
        println!(
            "{:>3} {mark} {}{}  ({}, {})",
            i + 1,
            f.name,
            f.version
                .as_deref()
                .map(|v| format!(" {v}"))
                .unwrap_or_default(),
            owner,
            Utf8Size(f.size)
        );
        println!("        {}", short(&f.path, &home));
        match &item.action {
            import::Action::Import => {}
            import::Action::Conflict { reason, alt_id } => println!(
                "        needs a decision: {reason}; --include-conflicts imports it as '{alt_id}'"
            ),
            import::Action::Skip(why) => println!("        not importable: {why}"),
            import::Action::Update { app_id, from, to } => println!(
                "        updates '{app_id}' from {} to {}",
                from.as_deref().unwrap_or("?"),
                to.as_deref().unwrap_or("?")
            ),
        }
        for n in &item.notes {
            println!("        - {n}");
        }
    }
    if !scan.skipped.is_empty() {
        println!("\nNot offered ({}):", scan.skipped.len());
        for (p, why) in &scan.skipped {
            println!("  {}  -- {why}", short(p, &home));
        }
    }
    println!();
    if selected.is_empty() {
        println!("Nothing selected to import.");
        return Ok(());
    }
    println!(
        "{} of {} selected for import.",
        selected.len(),
        plan.items.len()
    );
    if args.dry_run {
        println!("Dry run: nothing was changed.");
        return Ok(());
    }

    use std::io::{BufRead, IsTerminal, Write};
    if !args.yes {
        if !std::io::stdin().is_terminal() {
            println!(
                "Nothing was changed. Re-run with --yes to import the selected apps (or --dry-run to only look)."
            );
            return Ok(());
        }
        print!(
            "Import the {} selected app(s)? [y/N, or numbers like 1,3 to choose] ",
            selected.len()
        );
        std::io::stdout().flush().ok();
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line)?;
        match parse_answer(&line, plan.items.len()) {
            Answer::No => {
                println!("Nothing was changed.");
                return Ok(());
            }
            Answer::Selected => {}
            Answer::Numbers(nums) => {
                selected = nums
                    .into_iter()
                    .map(|n| n - 1)
                    .filter(|i| !matches!(plan.items[*i].action, import::Action::Skip(_)))
                    .collect();
            }
        }
    }

    println!();
    let mut failed = 0;
    let results = import::execute(&lib, &plan, &selected);
    for r in &results {
        match &r.outcome {
            Ok(notes) => println!("OK     {}  ({})", r.id, notes.join("; ")),
            Err(e) => {
                failed += 1;
                println!("FAILED {}  {}", short(&r.path, &home), e);
            }
        }
    }
    println!(
        "\nImported {} of {}.",
        results.len() - failed,
        results.len()
    );
    if plan.items.iter().enumerate().any(|(i, it)| {
        selected.contains(&i)
            && matches!(it.found.owner, import::Owner::Tool { .. })
            && it.replace_entries.is_empty()
    }) {
        println!(
            "Some apps now have two menu entries (yours and the other tool's). Run again with --replace-entries to back up the other tool's entries."
        );
    }
    print_warnings(&lib);
    if failed > 0 {
        bail!("{failed} app(s) could not be imported");
    }
    Ok(())
}

pub(crate) struct Utf8Size(u64);

impl std::fmt::Display for Utf8Size {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mb = self.0 as f64 / (1024.0 * 1024.0);
        if mb >= 1.0 {
            write!(f, "{mb:.1} MiB")
        } else {
            write!(f, "{} KiB", self.0 / 1024)
        }
    }
}
