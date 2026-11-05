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

fn main() {
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
