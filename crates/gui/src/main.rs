mod backend;
mod instance;
mod service;

use cxx_qt_lib::{QQmlApplicationEngine, QQuickStyle, QString, QUrl};

unsafe extern "C" {
    fn appofholding_set_palette(which: i32);
    fn appofholding_app_new(argc: i32, argv: *mut *mut std::ffi::c_char);
    fn appofholding_app_exec() -> i32;
}

/// Create the (widgets) application object from this process's arguments; see app.cpp.
fn create_application() {
    use std::os::unix::ffi::OsStringExt;
    // Qt keeps pointers into argv for the life of the program, so these are never freed.
    let args: Vec<*mut std::ffi::c_char> = std::env::args_os()
        .filter_map(|a| std::ffi::CString::new(a.into_vec()).ok())
        .map(std::ffi::CString::into_raw)
        .chain(std::iter::once(std::ptr::null_mut()))
        .collect();
    let argc = (args.len() - 1) as i32;
    let argv = Box::leak(args.into_boxed_slice()).as_mut_ptr();
    // SAFETY: argv holds argc valid C strings and a final null, and is never freed.
    unsafe { appofholding_app_new(argc, argv) };
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

/// When appOFholding already runs (often hidden in the tray), show that window instead of
/// opening a second one, and give it any files this start was asked to open.
fn hand_over_to_running_copy() {
    let Some(path) = instance::socket_path() else {
        return;
    };
    let req = instance::Request {
        files: backend::startup_file_list(),
        show: !backend::started_in_tray(),
        open: backend::startup_open(),
    };
    if instance::forward(&path, &req) {
        std::process::exit(0);
    }
}

fn main() {
    forward_cli_commands();
    hand_over_to_running_copy();
    create_application();
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
    // SAFETY: the application object exists (create_application).
    let code = unsafe { appofholding_app_exec() };
    drop(engine);
    std::process::exit(code);
}
