import QtQuick
import QtQuick.Window
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import QtQuick.Dialogs
import Qt.labs.platform as Platform
import org.kde.kirigami as Kirigami
import io.github.appofholding

Kirigami.ApplicationWindow {
    id: root

    title: "appOFholding"
    width: Math.min(Kirigami.Units.gridUnit * 52, Screen.desktopAvailableWidth * 0.95)
    height: Kirigami.Units.gridUnit * 38
    minimumWidth: Kirigami.Units.gridUnit * 22
    minimumHeight: Kirigami.Units.gridUnit * 20
    // Shown from Component.onCompleted, unless it starts in the tray.
    visible: false

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

    // Tray and startup settings (see SettingsPage); reloaded when they change.
    property var desk: Util.parse(backend.desktopSettings())
    // Whether the desktop has a system tray to put the icon in (not on plain GNOME). The tray
    // does not announce changes to this, so it is read at start and while waiting for the tray.
    property bool trayAvailable: false
    readonly property bool inTray: desk.tray === true && trayAvailable
    // Updates the background checks have announced, for the tray's tooltip.
    property int updateCount: 0

    function reloadDesktop() {
        var d = Util.parse(backend.desktopSettings())
        root.desk = d.error ? ({}) : d
    }

    Backend { id: backend }

    // ---- the window and the tray

    function showWindow() {
        if (root.visibility === Window.Minimized) root.showNormal()
        else root.show()
        root.raise()
        root.requestActivate()
    }

    function hideToTray() {
        root.hide()
    }

    // Closing the window keeps the program in the tray when asked to.
    onClosing: (close) => {
        if (root.inTray && root.desk.closeToTray) {
            close.accepted = false
            root.hideToTray()
        }
    }

    // Minimising hides the window in the tray. Wayland never tells a window it was minimised,
    // so there this only works through "close to tray" (the Settings page says so).
    onVisibilityChanged: (v) => {
        if (v === Window.Minimized && root.inTray && root.desk.minimiseToTray) root.hideToTray()
    }

    Platform.SystemTrayIcon {
        id: tray
        visible: root.desk.tray === true
        icon.name: "io.github.appofholding"
        icon.source: "qrc:/appofholding/app-icon.svg"
        tooltip: root.updateCount > 0 ? qsTr("appOFholding — %n update(s) available", "", root.updateCount) : "appOFholding"

        onActivated: (reason) => {
            if (reason === Platform.SystemTrayIcon.Context) return
            if (root.visible && root.active) root.hideToTray()
            else root.showWindow()
        }
        onMessageClicked: {
            root.showWindow()
            if (root.announcedApp) root.openApp(root.announcedApp)
        }

        menu: Platform.Menu {
            Platform.MenuItem {
                text: root.visible ? qsTr("Hide appOFholding") : qsTr("Show appOFholding")
                onTriggered: root.visible ? root.hideToTray() : root.showWindow()
            }
            Platform.MenuItem {
                text: qsTr("Check for updates")
                enabled: !root.working
                onTriggered: root.checkAll()
            }
            Platform.MenuSeparator {}
            Platform.MenuItem {
                text: qsTr("Quit")
                onTriggered: Qt.quit()
            }
        }
    }

    // While the tray icon is up, check for updates now and then and announce new versions.
    readonly property bool watchUpdates: root.inTray && root.desk.notifyUpdates === true
    // The app a single-update announcement is about, opened when the message is clicked.
    property string announcedApp

    function backgroundCheck() {
        if (root.working) {
            retryCheck.restart()   // something else is running; try again shortly
            return
        }
        root.manualCheck = false
        backend.start("notifyCheck", "", "", "", "")
    }

    Timer { id: firstCheck; interval: 2 * 60 * 1000; running: root.watchUpdates; onTriggered: root.backgroundCheck() }
    Timer { interval: 3 * 3600 * 1000; repeat: true; running: root.watchUpdates; onTriggered: root.backgroundCheck() }
    Timer { id: retryCheck; interval: 5 * 60 * 1000; onTriggered: if (root.watchUpdates) root.backgroundCheck() }

    function announce(fresh) {
        if (fresh.length === 0 || !tray.visible) return
        var title, body
        if (fresh.length === 1) {
            root.announcedApp = fresh[0].id
            title = qsTr("%1 %2 is available").arg(fresh[0].name).arg(fresh[0].version)
            body = qsTr("A new version of %1 has been released. Click to open it in appOFholding.").arg(fresh[0].name)
        } else {
            root.announcedApp = ""
            title = qsTr("%n app update(s) available", "", fresh.length)
            var lines = []
            for (var i = 0; i < fresh.length; i++) lines.push(fresh[i].name + " " + fresh[i].version)
            body = lines.join("\n")
        }
        tray.showMessage(title, body, Platform.SystemTrayIcon.Information, 10000)
    }

    // Started from the login autostart with --tray: the tray may come up a little after us, so
    // wait for it briefly before giving up and opening the window.
    Timer {
        id: waitForTray
        interval: 500
        repeat: true
        property int tries: 0
        onTriggered: {
            waitForTray.tries++
            root.trayAvailable = tray.available
            if (root.trayAvailable || waitForTray.tries >= 30) {
                waitForTray.stop()
                if (!root.inTray) root.showWindow()
            }
        }
    }

    function reload() {
        var r = Util.parse(backend.apps())
        apps = r.error ? [] : r
        updateCount = countUpdates(apps)
        if (r.error) showPassiveNotification(r.error)
        revision++
    }

    function countUpdates(list) {
        var n = 0
        for (var i = 0; i < list.length; i++) if (list[i].status === "update-available") n++
        return n
    }

    // Asked for by the user (from the window or the tray menu) rather than the background timer.
    property bool manualCheck: false

    function checkAll() {
        root.manualCheck = true
        backend.start("notifyCheck", "", "", "", "")
    }

    readonly property var appTabs: ["overview", "updates", "launch", "sandbox"]

    function openApp(id, tab) {
        while (pageStack.depth > 1) pageStack.pop()
        pageStack.push(Qt.resolvedUrl("AppPage.qml"), {appId: id, tabIndex: Math.max(0, root.appTabs.indexOf(tab || "overview"))})
    }

    // An app page asked for on the command line or by a later start: {app, tab} or null.
    function openTarget(target) {
        if (target && typeof target.app === "string" && target.app !== "") {
            root.showWindow()
            root.openApp(target.app, target.tab)
        }
    }

    Component.onCompleted: {
        reload()
        backend.listen()
        root.trayAvailable = tray.available
        if (backend.startedInTray() && root.desk.tray === true) {
            if (!root.trayAvailable) waitForTray.start()
        } else {
            root.showWindow()
        }
        // Files handed to us by the desktop (double-click, "Open with", drag onto the icon).
        var files = Util.parse(backend.startupFiles())
        root.addFiles(files)
        // An app page asked for on the command line (uQ's "Launch options…" app action).
        root.openTarget(Util.parse(backend.startupOpen()))
        backend.start("selfCheck", "", "", "", "")   // answered from a cache most of the time
    }

    Connections {
        target: backend

        function onProgress(id, fraction) {
            var p = Object.assign({}, root.progress)
            p[id] = fraction
            root.progress = p
        }

        // Someone started appOFholding again: show this window instead of a second one.
        function onActivated(files, show, target) {
            if (show) root.showWindow()
            var list = Util.parse(files)
            root.addFiles(list)
            root.openTarget(Util.parse(target))
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
            case "notifyCheck":
                root.reload()
                var manual = root.manualCheck
                root.manualCheck = false
                if (!ok) {
                    if (manual) root.showPassiveNotification(payload, "long")
                    break
                }
                var n = root.countUpdates(data.results)
                var summary = n > 0 ? qsTr("%n update(s) available", "", n) : qsTr("Everything is up to date")
                if (root.visible && root.active) {
                    // The user is looking at the window: say it here, not on the desktop.
                    if (manual || data.fresh.length > 0) root.showPassiveNotification(summary)
                } else if (data.fresh.length > 0) {
                    root.announce(data.fresh)
                } else if (manual && tray.visible) {
                    tray.showMessage("appOFholding", summary, Platform.SystemTrayIcon.Information, 5000)
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
                    root.showPassiveNotification(data.moved
                        ? qsTr("Added %1 and moved it to %2").arg(data.name).arg(backend.dataDir())
                        : qsTr("Added %1").arg(data.name))
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
            },
            // Closing the window only hides it then, so offer a way out here too.
            Kirigami.Action {
                text: qsTr("Quit")
                icon.name: "application-exit"
                visible: root.inTray && root.desk.closeToTray === true
                onTriggered: Qt.quit()
            }
        ]
    }

    pageStack.defaultColumnWidth: Kirigami.Units.gridUnit * 25
    pageStack.initialPage: AppsPage {}

    AddDialog { id: addDialog }

    // ---- adding AppImages: dropped on the window, opened with appOFholding, or chosen in a dialog

    // Add files (paths or file:// URLs). Ones already in the library folder are simply added;
    // for the others, the Settings choice says whether to move, copy or keep them, or to ask.
    function addFiles(list) {
        if (!list || list.length === 0) return
        var found = Util.parse(backend.describeDrop(JSON.stringify(list)))
        if (found.error) { root.showPassiveNotification(found.error, "long"); return }
        if (found.length === 0) {
            root.showPassiveNotification(qsTr("Nothing to add: drop AppImage files, not folders."))
            return
        }
        var outside = []
        for (var i = 0; i < found.length; i++) {
            if (found[i].inLibrary) backend.start("addFile", "", found[i].path, "move", "")
            else outside.push(found[i])
        }
        if (outside.length === 0) return
        var action = root.desk.dropAction || "ask"
        if (action === "ask") {
            placeDialog.files = outside
            placeDialog.open()
        } else {
            root.addWith(outside, action)
        }
    }

    function addWith(files, mode) {
        for (var i = 0; i < files.length; i++) backend.start("addFile", "", files[i].path, mode, "")
    }

    // Where a dropped AppImage should live.
    Kirigami.Dialog {
        id: placeDialog
        property var files: []
        readonly property string libDir: backend.dataDir()
        title: placeDialog.files.length === 1
            ? qsTr("Add %1").arg(placeDialog.files[0].name)
            : qsTr("Add %n AppImage(s)", "", placeDialog.files.length)
        padding: Kirigami.Units.largeSpacing
        preferredWidth: Kirigami.Units.gridUnit * 26
        standardButtons: Kirigami.Dialog.Cancel
        customFooterActions: [
            Kirigami.Action {
                text: qsTr("Add")
                icon.name: "list-add"
                onTriggered: {
                    var mode = moveChoice.checked ? "move" : copyChoice.checked ? "copy" : "in-place"
                    if (rememberChoice.checked) {
                        var d = Object.assign({}, root.desk)
                        delete d.error
                        d.dropAction = mode
                        var err = backend.setDesktopSettings(JSON.stringify(d))
                        if (err) root.showPassiveNotification(err, "long")
                        root.reloadDesktop()
                    }
                    root.addWith(placeDialog.files, mode)
                    placeDialog.close()
                }
            }
        ]
        onOpened: { moveChoice.checked = true; rememberChoice.checked = false }

        ColumnLayout {
            spacing: Kirigami.Units.smallSpacing
            QQC2.Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                visible: placeDialog.files.length > 1
                text: placeDialog.files.map(function (f) { return f.name }).join(", ")
                font.family: "monospace"
            }
            QQC2.Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                text: qsTr("Where should it live?")
                font.weight: Font.DemiBold
                Layout.topMargin: Kirigami.Units.smallSpacing
            }
            QQC2.RadioButton {
                id: moveChoice
                checked: true
                text: qsTr("Move it to %1 (recommended)").arg(placeDialog.libDir)
            }
            QQC2.RadioButton {
                id: copyChoice
                text: qsTr("Copy it to %1 and keep the original").arg(placeDialog.libDir)
            }
            QQC2.RadioButton {
                id: keepChoice
                text: qsTr("Leave it where it is")
            }
            QQC2.Label {
                Layout.fillWidth: true
                Layout.leftMargin: Kirigami.Units.gridUnit * 1.5
                wrapMode: Text.Wrap
                opacity: 0.7
                font: Kirigami.Theme.smallFont
                text: keepChoice.checked
                    ? qsTr("It stays your file: removing the app from appOFholding leaves it in place. Moving or renaming it later breaks the menu entry.")
                    : qsTr("appOFholding keeps it there with your other apps, and replaces it when an update comes.")
            }
            QQC2.CheckBox {
                id: rememberChoice
                Layout.topMargin: Kirigami.Units.smallSpacing
                text: qsTr("Do this every time (change it in Settings)")
            }
        }
    }

    // Drop AppImages anywhere on the window.
    DropArea {
        id: dropArea
        anchors.fill: parent
        z: 1000
        onEntered: (drag) => { if (!drag.hasUrls) drag.accepted = false }
        onDropped: (drop) => {
            if (!drop.hasUrls) return
            var urls = []
            for (var i = 0; i < drop.urls.length; i++) urls.push(drop.urls[i].toString())
            drop.acceptProposedAction()
            root.addFiles(urls)
        }
        Rectangle {
            anchors.fill: parent
            anchors.margins: Kirigami.Units.smallSpacing
            visible: dropArea.containsDrag
            color: Qt.rgba(Kirigami.Theme.highlightColor.r, Kirigami.Theme.highlightColor.g, Kirigami.Theme.highlightColor.b, 0.15)
            border.color: Kirigami.Theme.highlightColor
            border.width: 2
            radius: Kirigami.Units.cornerRadius
            ColumnLayout {
                anchors.centerIn: parent
                spacing: Kirigami.Units.smallSpacing
                Kirigami.Icon {
                    source: "document-import"
                    Layout.alignment: Qt.AlignHCenter
                    Layout.preferredWidth: Kirigami.Units.iconSizes.huge
                    Layout.preferredHeight: Kirigami.Units.iconSizes.huge
                }
                Kirigami.Heading {
                    level: 2
                    text: qsTr("Drop to add to your library")
                    Layout.alignment: Qt.AlignHCenter
                }
            }
        }
    }
    ImportDialog { id: importDialog }

    FileDialog {
        id: fileDialog
        title: qsTr("Choose an AppImage")
        nameFilters: [qsTr("AppImage files (*.AppImage *.appimage)"), qsTr("All files (*)")]
        onAccepted: root.addFiles([selectedFile.toString()])
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
