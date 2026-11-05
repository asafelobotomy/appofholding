mod backend;
mod service;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QQuickStyle, QString, QUrl};

unsafe extern "C" {
    fn appofholding_set_palette(which: i32);
}

/// Our own colour themes replace the application palette; "system" leaves the desktop's alone.
/// Needs the application to exist, and has to happen before the interface is loaded.
fn apply_theme() -> bool {
    let which = match backend::current_theme().as_str() {
        "gruvbox-dark" => 1,
        "gruvbox-light" => 2,
        _ => return false,
    };
    // SAFETY: plain C function defined in theme.cpp; the application object exists.
    unsafe { appofholding_set_palette(which) };
    true
}

/// When appOFholding is packaged as an AppImage, its menu entries and timer call the AppImage
/// file, which starts this binary: hand those command-line jobs to the CLI next to it.
fn forward_cli_commands() {
    use std::os::unix::process::CommandExt;
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let is_cli = args
        .first()
        .is_some_and(|a| a == "run" || a == "update" || a == "--cli");
    if !is_cli {
        return;
    }
    let Some(cli) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("appofholding")))
        .filter(|p| p.exists())
    else {
        return;
    };
    let rest = if args[0] == "--cli" {
        &args[1..]
    } else {
        &args[..]
    };
    let err = std::process::Command::new(cli).args(rest).exec();
    eprintln!("appofholding-gui: could not start the command-line tool: {err}");
    std::process::exit(127);
}

fn main() {
    forward_cli_commands();
    let mut app = QGuiApplication::new();
    // A custom palette needs a style that obeys it; the desktop style follows the system.
    let themed = apply_theme();
    QQuickStyle::set_style(&QString::from(if themed {
        "Fusion"
    } else {
        "org.kde.desktop"
    }));
    let mut engine = QQmlApplicationEngine::new();
    if let Some(e) = engine.as_mut() {
        e.load(&QUrl::from(
            "qrc:/qt/qml/io/github/appofholding/qml/Main.qml",
        ));
    }
    if let Some(a) = app.as_mut() {
        a.exec();
    }
}
