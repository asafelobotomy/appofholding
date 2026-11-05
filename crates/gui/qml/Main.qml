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
            delete p[id]
            root.progress = p
            var data = ok ? Util.parse(payload) : {error: payload}
            switch (kind) {
            case "selfCheck":
                if (ok) root.selfInfo = data
                break
            case "selfUpdate":
                if (!ok) {
                    root.showPassiveNotification(payload)
                } else if (data.result === "replaced") {
                    root.showPassiveNotification(qsTr("Updated to %1. Restart appOFholding to use it.").arg(data.to))
                    root.selfInfo = null
                } else if (data.result === "package") {
                    root.showPassiveNotification(qsTr("Run in a terminal: appofholding self update --install"))
                } else {
                    root.showPassiveNotification(data.instructions || qsTr("Nothing to update"))
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
                } else if (kind === "addFile") {
                    root.showPassiveNotification(payload)
                }
                break
            }
        }
    }

    globalDrawer: Kirigami.GlobalDrawer {
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

    function openAddDialog() { addDialog.openDialog() }
    function openImportDialog() { importDialog.openDialog() }
    function openFileDialog() { fileDialog.open() }
}
