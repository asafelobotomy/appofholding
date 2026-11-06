import QtQuick
import QtQuick.Window
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import QtQuick.Dialogs
import org.kde.kirigami as Kirigami
import io.github.appofholding

Kirigami.ApplicationWindow {
    id: root

    title: "appOFholding"
    width: Math.min(Kirigami.Units.gridUnit * 52, Screen.desktopAvailableWidth * 0.95)
    height: Kirigami.Units.gridUnit * 38
    minimumWidth: Kirigami.Units.gridUnit * 22
    minimumHeight: Kirigami.Units.gridUnit * 20

    property var apps: []
    // Bumped whenever the library changed, so open pages reload themselves.
    property int revision: 0
    property var progress: ({})
    // Result of the latest check for a new appOFholding release ({available, latest, install, ...}).
    property var selfInfo: null
    readonly property bool working: backend.tasks > 0
    readonly property alias backendObject: backend
    // The chosen colour theme ("system" follows the desktop); changing it takes effect after a restart.
    property string themeName: backend.theme()
    // Colours for our own theme as seen by Kirigami (the style's controls use the application palette).
    readonly property var themePalette: Themes.palette(themeName)
    // Also reaches the dialogs, which live on the window rather than on a page.
    readonly property ThemeBindings themeBindings: ThemeBindings { target: root; p: root.themePalette }

    Backend { id: backend }

    function reload() {
        var r = Util.parse(backend.apps())
        apps = r.error ? [] : r
        if (r.error) showPassiveNotification(r.error)
        revision++
    }

    function checkAll() {
        backend.start("checkAll", "", "", "", "")
    }

    function openApp(id) {
        while (pageStack.depth > 1) pageStack.pop()
        pageStack.push(Qt.resolvedUrl("AppPage.qml"), {appId: id})
    }

    Component.onCompleted: {
        reload()
        // Files handed to us by the desktop (double-click, "Open with", drag onto the icon).
        var files = Util.parse(backend.startupFiles())
        for (var i = 0; i < files.length; i++) backend.start("addFile", "", files[i], "", "")
        backend.start("selfCheck", "", "", "", "")   // answered from a cache most of the time
    }

    Connections {
        target: backend

        function onProgress(id, fraction) {
            var p = Object.assign({}, root.progress)
            p[id] = fraction
            root.progress = p
        }

        function onFinished(kind, id, ok, payload) {
            var p = Object.assign({}, root.progress)
            delete p[id !== "" ? id : "@" + kind]   // the key the backend reports progress under
            root.progress = p
            var data = ok ? Util.parse(payload) : {error: payload}
            switch (kind) {
            case "selfCheck":
                if (ok) root.selfInfo = data
                break
            case "selfUpdate":
                if (!ok) {
                    root.showPassiveNotification(payload, "long")
                } else if (data.result === "installed" || data.result === "replaced") {
                    root.selfInfo = null
                    restartDialog.version = data.to
                    restartDialog.open()
                } else if (data.result === "current") {
                    root.showPassiveNotification(qsTr("appOFholding is already up to date"))
                } else {
                    root.showPassiveNotification(data.instructions || qsTr("Nothing to update"), "long")
                }
                break
            case "checkAll":
                root.reload()
                if (ok) {
                    var n = 0
                    for (var i = 0; i < data.length; i++) if (data[i].status === "update-available") n++
                    root.showPassiveNotification(n > 0 ? qsTr("%1 update(s) available").arg(n) : qsTr("Everything is up to date"))
                }
                break
            case "check":
                root.reload()
                if (!ok) root.showPassiveNotification(payload)
                break
            case "update":
                root.reload()
                if (!ok) {
                    root.showPassiveNotification(payload)
                } else if (data.result === "updated") {
                    root.showPassiveNotification(qsTr("Updated to %1").arg(data.to))
                } else if (data.result === "none") {
                    root.showPassiveNotification(qsTr("Already up to date"))
                } else {
                    root.showPassiveNotification(qsTr("Not updated: %1").arg(data.why || data.result))
                }
                break
            case "addFile":
            case "install":
                root.reload()
                if (ok) {
                    root.showPassiveNotification(qsTr("Added %1").arg(data.name))
                    root.openApp(data.id)
                } else if (kind === "addFile" || !addDialog.visible) {
                    // The add dialog shows its own errors while it is open.
                    root.showPassiveNotification(payload)
                }
                break
            case "importRun":
                // Also when the import dialog was closed while it ran.
                root.reload()
                if (!ok && !importDialog.visible) root.showPassiveNotification(payload)
                break
            }
        }
    }

    globalDrawer: Kirigami.GlobalDrawer {
        id: drawer
        readonly property ThemeBindings themeBindings: ThemeBindings { target: drawer; p: root.themePalette }
        title: "appOFholding"
        titleIcon: "qrc:/appofholding/app-icon.svg"
        isMenu: false
        actions: [
            Kirigami.Action {
                text: qsTr("Apps")
                icon.name: "view-list-icons"
                onTriggered: { while (pageStack.depth > 1) pageStack.pop() }
            },
            Kirigami.Action {
                text: qsTr("Add from a link…")
                icon.name: "download"
                onTriggered: addDialog.openDialog()
            },
            Kirigami.Action {
                text: qsTr("Add from a file…")
                icon.name: "document-open"
                onTriggered: fileDialog.open()
            },
            Kirigami.Action {
                text: qsTr("Import existing AppImages…")
                icon.name: "document-import"
                onTriggered: importDialog.openDialog()
            },
            Kirigami.Action {
                text: qsTr("Check for updates")
                icon.name: "view-refresh"
                enabled: !root.working
                onTriggered: root.checkAll()
            },
            Kirigami.Action {
                text: qsTr("Settings")
                icon.name: "configure"
                onTriggered: {
                    while (pageStack.depth > 1) pageStack.pop()
                    pageStack.push(Qt.resolvedUrl("SettingsPage.qml"))
                }
            }
        ]
    }

    pageStack.defaultColumnWidth: Kirigami.Units.gridUnit * 25
    pageStack.initialPage: AppsPage {}

    AddDialog { id: addDialog }
    ImportDialog { id: importDialog }

    FileDialog {
        id: fileDialog
        title: qsTr("Choose an AppImage")
        nameFilters: [qsTr("AppImage files (*.AppImage *.appimage)"), qsTr("All files (*)")]
        onAccepted: backend.start("addFile", "", selectedFile.toString(), "", "")
    }

    function confirmSelfUpdate() { selfUpdateDialog.open() }

    // Updating appOFholding itself: say exactly what will happen before doing it.
    Kirigami.PromptDialog {
        id: selfUpdateDialog
        readonly property var info: root.selfInfo
        title: info ? qsTr("Update to appOFholding %1?").arg(info.latest) : ""
        subtitle: !info ? ""
            : info.install === "pacman"
              ? qsTr("The package is downloaded from the GitHub release, checked against its published checksum, and installed with pacman. Your system will ask for your password.")
              : qsTr("The new AppImage is downloaded from the GitHub release, checked against its published checksum, and put in place of this one. The current version is kept beside it.")
        standardButtons: Kirigami.Dialog.Cancel
        customFooterActions: [
            Kirigami.Action {
                text: qsTr("Update")
                icon.name: "update-none"
                onTriggered: {
                    selfUpdateDialog.close()
                    backend.start("selfUpdate", "", "", "", "")
                }
            },
            Kirigami.Action {
                text: qsTr("What's new")
                icon.name: "help-about"
                onTriggered: Qt.openUrlExternally(selfUpdateDialog.info.url)
            }
        ]
    }

    Kirigami.PromptDialog {
        id: restartDialog
        property string version
        title: qsTr("appOFholding %1 is installed").arg(version)
        subtitle: qsTr("Restart to start using it. This window keeps running the old version until then.")
        standardButtons: Kirigami.Dialog.NoButton
        customFooterActions: [
            Kirigami.Action {
                text: qsTr("Restart now")
                icon.name: "view-refresh"
                onTriggered: {
                    var err = backend.relaunch()
                    if (err) root.showPassiveNotification(err, "long")
                    else Qt.quit()
                }
            },
            Kirigami.Action {
                text: qsTr("Later")
                icon.name: "dialog-cancel"
                onTriggered: restartDialog.close()
            }
        ]
    }

    function openAddDialog() { addDialog.openDialog() }
    function openImportDialog() { importDialog.openDialog() }
    function openFileDialog() { fileDialog.open() }
}
