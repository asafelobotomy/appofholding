mod backend;
mod service;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QQuickStyle, QString, QUrl};

fn main() {
    let mut app = QGuiApplication::new();
    QQuickStyle::set_style(&QString::from("org.kde.desktop"));
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
