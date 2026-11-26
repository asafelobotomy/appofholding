//! `check` and `update`: look for new versions and install them.

use super::*;

pub(crate) fn check_cmd(cmd: Cmd) -> Result<()> {
    let Cmd::Check {
        id,
        desktop_id,
        json,
        notify,
        ndjson,
    } = cmd
    else {
        unreachable!()
    };
    let lib = library()?;
    let http = UreqHttp::new();
    let id = match desktop_id {
        Some(d) => Some(app_for_desktop_id(&lib, &d)?),
        None => id,
    };
    let ids: Vec<String> = match id {
        Some(i) => vec![i],
        None => lib.sources()?.into_iter().map(|s| s.app_id).collect(),
    };
    if ndjson {
        // A few apps at a time (as `check_many` does), a line after each batch.
        pagejson::event(&json!({ "event": "start", "total": ids.len() }));
        let mut updates = 0;
        let mut done = 0;
        for batch in ids.chunks(4) {
            let results = lib.check_many(&http, batch);
            let sources = lib.sources()?;
            for (i, result) in &results {
                let src = sources.iter().find(|s| &s.app_id == i);
                let row = check_row(i, result, src);
                updates += usize::from(row["update"] == true);
                done += 1;
                let mut ev = json!({ "event": "checked", "index": done, "total": ids.len() });
                ev.as_object_mut()
                    .unwrap()
                    .extend(row.as_object().cloned().unwrap_or_default());
                pagejson::event(&ev);
            }
        }
        pagejson::event(&json!({ "event": "done", "updates": updates, "total": ids.len() }));
        return Ok(());
    }
    let results = lib.check_many(&http, &ids);
    // Read after the check, so a pin or skip is as current as the answer.
    let sources = lib.sources()?;
    if notify {
        for (i, result) in &results {
            let name = lib.get(i).map(|a| a.name).unwrap_or_else(|_| i.clone());
            let src = sources.iter().find(|s| &s.app_id == i);
            NotifySend.notify(&name, &check_summary(result, src));
        }
    }
    if json {
        println!("{}", check_json(&results, &sources));
        return Ok(());
    }
    if ids.is_empty() {
        println!("No apps have an update source yet. Use `appofholding source set <id> <link>`.");
    }
    let mut failed = 0;
    for (i, result) in results {
        match result {
            Ok(r) => match r.problem() {
                Some(p) => println!("{i:<24} problem: {p}"),
                None => println!(
                    "{i:<24} {:<16} latest {}",
                    r.status.as_str(),
                    r.latest_version()
                ),
            },
            Err(e) => {
                failed += 1;
                println!("{i:<24} error: {e}");
            }
        }
    }
    if failed > 0 {
        bail!("{failed} check(s) failed");
    }
    Ok(())
}

pub(crate) fn update_cmd(cmd: Cmd) -> Result<()> {
    let Cmd::Update {
        id,
        scheduled,
        force,
        defer_if_running,
        keep,
        from,
        copy,
        dry_run,
        json,
        ndjson,
    } = cmd
    else {
        unreachable!()
    };
    if dry_run {
        let plan = library().and_then(|lib| pagejson::update_plan(&lib, id.as_deref(), force));
        if json {
            json_or_exit(plan);
        } else {
            let plan = plan?;
            println!("{}", plan["summary"].as_str().unwrap_or(""));
            for c in plan["changes"].as_array().into_iter().flatten() {
                println!(
                    "  {:<24} {}",
                    c["label"].as_str().unwrap_or(""),
                    c["detail"].as_str().unwrap_or("")
                );
            }
        }
        return Ok(());
    }
    let lib = library()?;
    if let (Some(file), Some(id)) = (&from, &id) {
        let found = lib.local_version_of(file)?;
        match found.as_ref().map(|f| (f.id.as_str(), f.relation)) {
            Some((fid, _)) if fid != id => {
                bail!("{} is a version of '{fid}', not of '{id}'", file.display())
            }
            None => bail!("{} is not a version of '{id}'", file.display()),
            Some((_, engine::Relation::Same)) if !force => bail!(
                "{} is the version already installed (--force installs it anyway)",
                file.display()
            ),
            Some((_, engine::Relation::Older)) if !force => bail!(
                "{} is older than the installed version (--force installs it anyway)",
                file.display()
            ),
            _ => {}
        }
        let mode = if copy { Install::Copy } else { Install::Move };
        let from_v = found.and_then(|f| f.installed);
        let app = lib.update_from_file(id, file, mode)?;
        println!(
            "{id}: {} -> {}",
            from_v.as_deref().unwrap_or("?"),
            app.version.as_deref().unwrap_or("?")
        );
        for w in lib.take_warnings() {
            eprintln!("warning: {w}");
        }
        return Ok(());
    }
    let http = UreqHttp::new();
    if scheduled {
        for o in lib.run_scheduled(&http, &NotifySend)? {
            println!("{}", describe_outcome(&o));
        }
        // The program itself: tell the user once when a new release is out.
        match selfupdate::notify_if_new(
            &http,
            &NotifySend,
            &lib.dirs,
            selfupdate::detect_current(),
            Arch::host(),
        ) {
            Ok(Some(msg)) => println!("appofholding: {msg}"),
            Ok(None) => {}
            Err(e) => println!("appofholding: could not check for a new release: {e}"),
        }
        return Ok(());
    }
    let opts = ApplyOptions {
        if_running: if defer_if_running {
            IfRunning::Defer
        } else {
            IfRunning::Proceed
        },
        keep_versions: match keep {
            Some(n) => n,
            None => lib.keep_versions()?,
        },
        force,
    };
    let ids: Vec<String> = match id {
        Some(i) => vec![i],
        None => lib.sources()?.into_iter().map(|s| s.app_id).collect(),
    };
    if ids.is_empty() {
        println!("No apps have an update source yet. Use `appofholding source set <id> <link>`.");
    }
    if ndjson {
        return update_ndjson(&lib, &http, &ids, &opts);
    }
    let mut failed = false;
    for i in ids {
        match lib.update(&http, &i, &opts, &mut progress()) {
            Ok(Applied::Updated { from, to, delta }) => println!(
                "{i}: updated {} -> {to}{}",
                from.as_deref().unwrap_or("?"),
                if delta { " (delta)" } else { "" }
            ),
            Ok(Applied::NoUpdate(Status::Unknown)) => println!(
                "{i}: can't tell if there is an update (use --force to reinstall the latest)"
            ),
            Ok(Applied::NoUpdate(_)) => println!("{i}: already up to date"),
            Ok(Applied::Skipped(why)) => println!("{i}: skipped ({why})"),
            Ok(Applied::Deferred { version, pids }) => println!(
                "{i}: {version} is available but the app is running (pid {}); try again after closing it",
                pids.iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Err(e) => {
                failed = true;
                println!("{i}: error: {e}");
            }
        }
    }
    print_warnings(&lib);
    if failed {
        bail!("some updates failed");
    }
    Ok(())
}
