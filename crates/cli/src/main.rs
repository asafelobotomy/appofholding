use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, bail};
use appimgr_core::{AddOptions, Dirs, Install, Library, inspect};
use clap::{Parser, Subcommand};

/// Manage AppImages: integrate them into the desktop, track their update sources, and launch them.
#[derive(Parser)]
#[command(name = "appimgr", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Add an AppImage to the library and integrate it into the desktop menu.
    Add {
        file: PathBuf,
        /// Move the file into the library instead of copying it.
        #[arg(long = "move")]
        move_file: bool,
        /// Use this id instead of one derived from the app name.
        #[arg(long)]
        id: Option<String>,
    },
    /// List apps in the library.
    List,
    /// Show details about an AppImage file or a library app id.
    Info { target: String },
    /// Remove an app: its menu entry, icon and (unless --keep-file) the AppImage.
    Remove {
        id: String,
        /// Keep the AppImage file on disk.
        #[arg(long)]
        keep_file: bool,
    },
    /// Launch an app (this is what generated menu entries call).
    Run {
        id: String,
        /// Arguments passed to the application.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

fn dirs() -> Result<Dirs> {
    // APPIMGR_ROOT redirects everything into one directory (used by tests and for experiments).
    match std::env::var_os("APPIMGR_ROOT") {
        Some(root) => Ok(Dirs::with_root(&PathBuf::from(root))),
        None => Ok(Dirs::from_env()?),
    }
}

fn library() -> Result<Library> {
    let exe = std::env::current_exe().context("cannot determine own path")?;
    Ok(Library::open(dirs()?, exe)?)
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Cmd::Add {
            file,
            move_file,
            id,
        } => {
            let opts = AddOptions {
                install: if move_file {
                    Install::Move
                } else {
                    Install::Copy
                },
                id,
            };
            let rec = library()?.add(&file, &opts)?;
            println!("Added {} ({})", rec.name, rec.id);
            println!("  file: {}", rec.appimage_path.display());
            println!("  menu: {}", rec.desktop_file.display());
        }
        Cmd::List => {
            let apps = library()?.list()?;
            if apps.is_empty() {
                println!("No apps yet. Add one with: appimgr add <file>");
            }
            for a in apps {
                println!(
                    "{:<24} {:<28} {:<10} {}",
                    a.id,
                    a.name,
                    a.version.as_deref().unwrap_or("-"),
                    a.appimage_path.display()
                );
            }
        }
        Cmd::Info { target } => {
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
        }
        Cmd::Remove { id, keep_file } => {
            let rec = library()?.remove(&id, keep_file)?;
            println!("Removed {} ({})", rec.name, rec.id);
        }
        Cmd::Run { id, args } => {
            let rec = library()?.get(&id)?;
            // M4 will add launch profiles and sandboxing here; for now just exec the AppImage.
            let err = Command::new(&rec.appimage_path).args(args).exec();
            bail!("failed to launch {}: {err}", rec.appimage_path.display());
        }
    }
    Ok(())
}
