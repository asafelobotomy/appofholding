use std::path::{Path, PathBuf};
use std::process::Command;

use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};

/// Compile the QML to bytecode only.
///
/// By default qmlcachegen also turns QML into C++ ("ahead-of-time" compilation). That C++ calls
/// Qt's *private* API, which is tied to the exact Qt version it was built with: a package built
/// against Qt 6.12 then fails to start on Qt 6.11 with "undefined symbol ... Qt_6_PRIVATE_API"
/// (Arch and Manjaro are routinely on different Qt versions). Bytecode needs no private symbols,
/// and if its format ever does not match the installed Qt, Qt compiles the embedded QML source
/// instead, so the binary keeps working across Qt updates.
///
/// cxx-qt-build has no switch for this, but it finds qmlcachegen through `qmake -query`, and
/// honours `$QMAKE`. So this points `$QMAKE` at a small wrapper whose tool directory holds the
/// real Qt tools, except for a qmlcachegen that adds `--only-bytecode`.
fn bytecode_only_qmlcachegen() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    println!("cargo::rerun-if-env-changed=QMAKE");
    let real_qmake = std::env::var_os("QMAKE").map(PathBuf::from).or_else(|| {
        let path = std::env::var_os("PATH")?;
        ["qmake6", "qmake"].iter().find_map(|name| {
            std::env::split_paths(&path)
                .map(|d| d.join(name))
                .find(|p| p.is_file())
        })
    });
    let Some(real_qmake) = real_qmake else {
        return; // let cxx-qt-build report the missing Qt
    };
    let query = |var: &str| -> Option<PathBuf> {
        let out = Command::new(&real_qmake)
            .args(["-query", var])
            .output()
            .ok()?;
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (out.status.success() && !s.is_empty()).then(|| PathBuf::from(s))
    };
    let Some(libexec) = query("QT_HOST_LIBEXECS").or_else(|| query("QT_INSTALL_LIBEXECS")) else {
        return;
    };
    let real_cachegen = libexec.join("qmlcachegen");
    if !real_cachegen.is_file() {
        return;
    }

    let out =
        PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR")).join("qt-tools");
    let tools = out.join("libexec");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&tools).expect("create the Qt tool directory");
    for entry in std::fs::read_dir(&libexec)
        .expect("read the Qt tool directory")
        .flatten()
    {
        if entry.file_name() != "qmlcachegen" {
            symlink(entry.path(), tools.join(entry.file_name())).expect("link a Qt tool");
        }
    }
    // Tools are run with an empty environment, so everything here is an absolute path.
    let script = |path: &Path, body: String| {
        std::fs::write(path, format!("#!/bin/sh\n{body}\n")).expect("write a wrapper");
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .expect("make a wrapper executable");
    };
    let q = |p: &Path| format!("'{}'", p.display().to_string().replace('\'', "'\\''"));
    script(
        &tools.join("qmlcachegen"),
        format!("exec {} --only-bytecode \"$@\"", q(&real_cachegen)),
    );
    let wrapper = out.join("qmake");
    script(
        &wrapper,
        format!(
            "if [ \"$1\" = -query ]; then\n  case \"$2\" in\n    QT_HOST_LIBEXECS|QT_HOST_LIBEXECS/get|QT_INSTALL_LIBEXECS|QT_INSTALL_LIBEXECS/get)\n      echo {}; exit 0 ;;\n  esac\nfi\nexec {} \"$@\"",
            q(&tools),
            q(&real_qmake)
        ),
    );
    // SAFETY: the build script is single-threaded at this point.
    unsafe { std::env::set_var("QMAKE", &wrapper) };
}

fn main() {
    bytecode_only_qmlcachegen();
    CxxQtBuilder::new_qml_module(
        QmlModule::new("io.github.appofholding")
            .version(1, 0)
            .qml_files(
                [
                    "qml/Main.qml",
                    "qml/AppsPage.qml",
                    "qml/AppCard.qml",
                    "qml/AppPage.qml",
                    "qml/OverviewTab.qml",
                    "qml/SourceSection.qml",
                    "qml/LaunchSection.qml",
                    "qml/SandboxSection.qml",
                    "qml/AddDialog.qml",
                    "qml/ImportDialog.qml",
                    "qml/SettingsPage.qml",
                    "qml/Section.qml",
                    "qml/Pill.qml",
                    "qml/PrimaryButton.qml",
                    "qml/AppTile.qml",
                    "qml/AppRow.qml",
                    "qml/AppGridCard.qml",
                    "qml/AppMenuButton.qml",
                    "qml/SectionCard.qml",
                    "qml/ThemedPage.qml",
                    "qml/ThemeBindings.qml",
                ]
                .map(QmlFile::from)
                .into_iter()
                .chain([
                    QmlFile::from("qml/Util.qml").singleton(true),
                    QmlFile::from("qml/Themes.qml").singleton(true),
                ]),
            ),
    )
    .qrc("qml/resources/resources.qrc")
    .qt_module("Quick")
    .qt_module("QuickControls2")
    .file("src/backend.rs")
    .cpp_file("src/theme.cpp")
    .build();
}
