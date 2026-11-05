use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("io.github.appofholding")
            .version(1, 0)
            .qml_files(
                [
                    "qml/Main.qml",
                    "qml/AppsPage.qml",
                    "qml/AppCard.qml",
                    "qml/AppPage.qml",
                    "qml/SourceSection.qml",
                    "qml/LaunchSection.qml",
                    "qml/SandboxSection.qml",
                    "qml/AddDialog.qml",
                    "qml/SettingsPage.qml",
                    "qml/Section.qml",
                ]
                .map(QmlFile::from)
                .into_iter()
                .chain([QmlFile::from("qml/Util.qml").singleton(true)]),
            ),
    )
    .qt_module("Quick")
    .qt_module("QuickControls2")
    .file("src/backend.rs")
    .build();
}
