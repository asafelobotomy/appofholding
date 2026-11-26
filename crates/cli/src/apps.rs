//! `add`, `suggest`, `info`, `remove` and `apply`: an app's place in the library.

use super::*;

pub(crate) fn add_cmd(cmd: Cmd) -> Result<()> {
    let Cmd::Add {
        target,
        asset,
        channel,
        move_file,
        id,
        post_install,
        json,
    } = cmd
    else {
        unreachable!()
    };
    if json {
        json_or_exit(add_json(
            target,
            asset,
            channel,
            move_file,
            id,
            post_install,
        ));
        return Ok(());
    }
    let opts = AddOptions {
        install: if move_file {
            Install::Move
        } else {
            Install::Copy
        },
        id,
    };
    let lib = library()?;
    let path = PathBuf::from(&target);
    let rec = if path.is_file() {
        if asset.is_some() || channel.is_some() {
            bail!(
                "--asset and --channel only apply when adding from a link; \
                     for a local file, set them later with `appofholding source set`"
            );
        }
        if opts.id.is_none()
            && let Some(v) = lib.local_version_of(&path)?
        {
            let what = match v.relation {
                engine::Relation::Newer => "a newer version of",
                engine::Relation::Older => "an older version of",
                engine::Relation::Same => "the version already installed of",
                engine::Relation::Unknown => "another build of",
            };
            bail!(
                "{} is {what} '{}', which is already in the library; \
                     use `appofholding update {} --from {}` to install it",
                path.display(),
                v.id,
                v.id,
                path.display()
            );
        }
        lib.add(&path, &opts)?
    } else {
        let parsed = build_source(&target, asset, channel)?;
        println!("Resolving {} ...", parsed.source.describe());
        let (rec, report) =
            lib.install_from_source(&UreqHttp::new(), parsed, &opts, &mut progress())?;
        println!(
            "Downloaded {} ({})",
            report.asset().map_or("?", |a| a.name.as_str()),
            report.release.tag
        );
        rec
    };
    println!("Added {} ({})", rec.name, rec.id);
    println!("  file: {}", rec.appimage_path.display());
    println!("  menu: {}", rec.desktop_file.display());
    if let Some(s) = lib.source(&rec.id)? {
        println!("  updates: {}", s.source.describe());
    }
    if let Some(cmd) = post_install {
        lib.set_hook(&rec.id, "post-install", &cmd)?;
        lib.run_hook(&rec.id, "post-install")?;
        println!("  post-install hook ran");
    }
    Ok(())
}

pub(crate) fn suggest_cmd(cmd: Cmd) -> Result<()> {
    let Cmd::Suggest { id, apply, dismiss } = cmd else {
        unreachable!()
    };
    let lib = library()?;
    if let (Some(id), Some(key)) = (&id, &apply) {
        println!("{id}: {}", lib.apply_suggestion(&UreqHttp::new(), id, key)?);
        return Ok(());
    }
    if let (Some(id), Some(key)) = (&id, &dismiss) {
        lib.dismiss_suggestion(id, key)?;
        println!("{id}: '{key}' will not be suggested again.");
        return Ok(());
    }
    let host = appofholding_core::suggest::Host::detect();
    let ids: Vec<String> = match id {
        Some(i) => vec![i],
        None => lib.list()?.into_iter().map(|a| a.id).collect(),
    };
    let mut any = false;
    for i in &ids {
        match lib.suggestions(i, &host) {
            Ok(list) => {
                for s in list {
                    any = true;
                    println!("{i}: {}  [{}]\n    {}", s.title, s.key, s.detail);
                }
            }
            Err(e) => println!("{i}: could not check: {e}"),
        }
    }
    if !any {
        println!("No suggestions.");
    } else {
        println!("\nApply one with: appofholding suggest <id> --apply <key>");
    }
    Ok(())
}

pub(crate) fn info_cmd(cmd: Cmd) -> Result<()> {
    let Cmd::Info { target } = cmd else {
        unreachable!()
    };
    let path = PathBuf::from(&target);
    if path.is_file() {
        let i = inspect(&path)?;
        println!("name:        {}", i.name);
        println!("id:          {}", i.id);
        println!("version:     {}", i.version.as_deref().unwrap_or("-"));
        println!("type:        {}", i.image.kind);
        println!("desktop:     {}", i.desktop_path);
        println!(
            "update info: {}",
            i.image.update_info.as_deref().unwrap_or("-")
        );
        println!(
            "icon:        {}",
            match &i.icon {
                Some(ic) => format!(
                    "{:?} {}",
                    ic.format,
                    ic.size.map_or("scalable".into(), |s| format!("{s}px"))
                ),
                None => "none".into(),
            }
        );
    } else {
        let a = library()?.get(&target)?;
        println!("name:        {}", a.name);
        println!("id:          {}", a.id);
        println!("version:     {}", a.version.as_deref().unwrap_or("-"));
        println!("file:        {}", a.appimage_path.display());
        println!("sha256:      {}", a.sha256);
        println!("size:        {} bytes", a.size);
        println!("update info: {}", a.update_info.as_deref().unwrap_or("-"));
    }
    Ok(())
}

pub(crate) fn remove_cmd(cmd: Cmd) -> Result<()> {
    let Cmd::Remove {
        id,
        keep_file,
        delete_file,
        purge_data,
        dry_run,
        json,
    } = cmd
    else {
        unreachable!()
    };
    let policy = if delete_file {
        FilePolicy::Delete
    } else if keep_file {
        FilePolicy::Keep
    } else {
        FilePolicy::Auto
    };
    if dry_run {
        let plan = library().and_then(|lib| pagejson::remove_plan(&lib, &id, policy, purge_data));
        if json {
            json_or_exit(plan);
        } else {
            let plan = plan?;
            println!("{}", plan["summary"].as_str().unwrap_or(""));
            for c in plan["changes"].as_array().into_iter().flatten() {
                println!(
                    "  {:<14} {}",
                    c["group"].as_str().unwrap_or(""),
                    c["detail"].as_str().unwrap_or("")
                );
            }
        }
        return Ok(());
    }
    let done = (|| -> Result<serde_json::Value> {
        let lib = library()?;
        let rec = lib.remove_with(&id, policy)?;
        let kept_file = rec
            .appimage_path
            .exists()
            .then(|| rec.appimage_path.display().to_string());
        let home = lib.dirs.private_home(&id);
        let purged = purge_data && lib.purge_private_home(&id)?;
        let kept_data = !purged && home.is_dir();
        if !json {
            println!("Removed {} ({})", rec.name, rec.id);
            if let Some(f) = &kept_file {
                println!("The AppImage file was kept: {f}");
            }
            if purged {
                println!("Deleted its private data folder.");
            } else if kept_data {
                println!(
                    "Its private data folder was kept: {}\n(add --purge-data to delete it)",
                    home.display()
                );
            }
        }
        Ok(json!({
            "version": 1,
            "ok": true,
            "id": rec.id,
            "name": rec.name,
            "kept_file": kept_file,
            "kept_data": kept_data,
            "purged_data": purged,
        }))
    })();
    if json {
        json_or_exit(done);
    } else {
        done?;
    }
    Ok(())
}

pub(crate) fn apply_cmd(cmd: Cmd) -> Result<()> {
    let Cmd::Apply {
        file,
        no_install,
        prune,
        dry_run,
        allow_commands,
    } = cmd
    else {
        unreachable!()
    };
    let text = std::fs::read_to_string(&file)
        .with_context(|| format!("cannot read {}", file.display()))?;
    let cfg = Config::parse(&text)?;
    let lib = library()?;
    if dry_run {
        for a in &cfg.apps {
            let what = if lib.get(&a.id).is_ok() {
                "configure"
            } else if a.file.is_some() {
                "add from file, then configure"
            } else if a.source.is_some() && !no_install {
                "download, then configure"
            } else {
                "SKIP (not installed, nothing says how to get it)"
            };
            println!("{:<24} {what}", a.id);
            for c in appofholding_core::config::commands_in(a) {
                println!(
                    "{:<24}   runs a command: {c}{}",
                    "",
                    if allow_commands {
                        ""
                    } else {
                        "  (refused without --allow-commands)"
                    }
                );
            }
        }
        return Ok(());
    }
    let mut failed = false;
    for n in lib.apply_config(
        &UreqHttp::new(),
        &cfg,
        &ConfigApply {
            no_install,
            prune,
            allow_commands,
        },
    ) {
        match n {
            Note::Installed(id) => println!("{id}: downloaded and installed"),
            Note::Added(id) => println!("{id}: added from file"),
            Note::Configured(id) => println!("{id}: configured"),
            Note::Missing { id, reason } => println!("{id}: skipped ({reason})"),
            Note::Failed { id, error } => {
                failed = true;
                println!("{id}: FAILED: {error}");
            }
        }
    }
    print_warnings(&lib);
    if failed {
        bail!("some entries could not be applied");
    }
    Ok(())
}
