import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// Settings, in tabs: General, Updates, App defaults, Tray & startup, About. Every control saves
// as soon as it is changed; actions that change many apps at once ask first.
ThemedPage {
    id: page
    title: qsTr("Settings")
    readonly property var win: applicationWindow()
    readonly property var backend: win.backendObject
    property bool scheduled: false
    property string scheduledInterval: "daily"
    // Which tab is open; others can ask for one by name (see Main.openSettings).
    property alias tabIndex: tabs.currentIndex
    readonly property var tabNames: ["general", "updates", "defaults", "tray", "about"]

    function load() {
        var st = Util.parse(backend.scheduleStatus())
        scheduled = !!st.installed
        if (st.interval) scheduledInterval = st.interval
    }
    Component.onCompleted: load()

    // Tray and startup settings (gui-desktop.json).
    readonly property var desk: win.desk
    readonly property bool trayOn: desk.tray === true
    readonly property bool wayland: Qt.platform.pluginName.indexOf("wayland") >= 0
    function saveDesk(key, value) {
        var d = Object.assign({}, win.desk)
        delete d.error
        d[key] = value
        var err = backend.setDesktopSettings(JSON.stringify(d))
        if (err) win.showPassiveNotification(err, "long")
        win.reloadDesktop()
    }

    // Library-wide defaults (kept in the library, so the CLI and the timer follow them too).
    readonly property var defs: win.libDefaults
    function saveDefault(key, value) {
        var d = {}
        d[key] = value
        var err = backend.setLibraryDefaults(JSON.stringify(d))
        if (err) win.showPassiveNotification(err, "long")
        win.reloadDefaults()
    }

    readonly property var policyKeys: ["auto", "notify", "manual", "pinned"]
    readonly property var policyLabels: [
        qsTr("Install it automatically"),
        qsTr("Tell me"),
        qsTr("Stay quiet, I'll update myself"),
        qsTr("Never change the app")
    ]

    header: QQC2.TabBar {
        id: tabs
        QQC2.TabButton { text: qsTr("General"); icon.name: "configure" }
        QQC2.TabButton { text: qsTr("Updates"); icon.name: "view-refresh" }
        QQC2.TabButton { text: qsTr("App defaults"); icon.name: "view-list-icons" }
        QQC2.TabButton { text: qsTr("Tray & startup"); icon.name: "preferences-desktop" }
        QQC2.TabButton { text: qsTr("About"); icon.name: "help-about" }
    }

    ColumnLayout {
        spacing: Kirigami.Units.largeSpacing

        // ================= General
        ColumnLayout {
            visible: tabs.currentIndex === 0
            Layout.fillWidth: true
            spacing: Kirigami.Units.largeSpacing

            SectionCard {
                title: qsTr("Appearance")
                iconName: "preferences-desktop-color"
                subtitle: qsTr("Changes apply the next time appOFholding starts.")
                QQC2.ComboBox {
                    Layout.fillWidth: true
                    Layout.maximumWidth: Kirigami.Units.gridUnit * 24
                    model: Themes.labels
                    currentIndex: Math.max(0, Themes.names.indexOf(page.win.themeName))
                    Accessible.name: qsTr("Colour theme")
                    onActivated: {
                        var err = page.backend.setTheme(Themes.names[currentIndex])
                        page.win.showPassiveNotification(err || qsTr("Restart appOFholding to see the new theme."))
                    }
                }
            }

            SectionCard {
                title: qsTr("Library folder")
                iconName: "folder-appimage"
                subtitle: qsTr("Where appOFholding keeps the AppImages it manages.")
                RowLayout {
                    Layout.fillWidth: true
                    QQC2.Label {
                        text: page.win.dataDir
                        font.family: "monospace"
                        wrapMode: Text.WrapAnywhere
                        Layout.fillWidth: true
                    }
                    QQC2.Button {
                        text: qsTr("Open")
                        icon.name: "document-open-folder"
                        onClicked: Qt.openUrlExternally("file://" + page.win.dataDir.split("/").map(encodeURIComponent).join("/"))
                    }
                }
            }

            SectionCard {
                title: qsTr("Adding AppImages")
                iconName: "list-add"
                subtitle: qsTr("What happens to an AppImage you drop on the window or open with appOFholding. Files already in the library folder are always just added.")
                QQC2.ComboBox {
                    readonly property var keys: ["ask", "move", "copy", "in-place"]
                    Layout.fillWidth: true
                    Layout.maximumWidth: Kirigami.Units.gridUnit * 24
                    model: [
                        qsTr("Ask each time"),
                        qsTr("Move it into the library folder"),
                        qsTr("Copy it into the library folder"),
                        qsTr("Leave it where it is")
                    ]
                    currentIndex: Math.max(0, keys.indexOf(page.desk.dropAction))
                    onActivated: (i) => page.saveDesk("dropAction", keys[i])
                    Accessible.name: qsTr("Adding AppImages")
                }
            }
        }

        // ================= Updates
        ColumnLayout {
            visible: tabs.currentIndex === 1
            Layout.fillWidth: true
            spacing: Kirigami.Units.largeSpacing

            SectionCard {
                id: scheduleCard
                title: qsTr("Background checks")
                iconName: "chronometer"
                subtitle: qsTr("A system timer checks your apps even when appOFholding is closed. It installs updates for apps set to install automatically, and sends one notification per new version for apps set to “Tell me”.")

                // Turning the timer on or off runs systemctl, so it is a task; the controls wait for
                // its answer and then show what is really set up.
                property bool busy: false
                Connections {
                    target: page.backend
                    function onFinished(kind, id, ok, payload) {
                        if (kind !== "scheduleInstall" && kind !== "scheduleRemove") return
                        scheduleCard.busy = false
                        if (!ok) page.win.showPassiveNotification(payload, "long")
                        page.load()
                        // A toggle or pick replaced the bindings; follow the real state again.
                        sw.checked = Qt.binding(() => page.scheduled)
                        interval.currentIndex = Qt.binding(() => Math.max(0, interval.keys.indexOf(page.scheduledInterval)))
                    }
                }

                RowLayout {
                    enabled: !scheduleCard.busy
                    spacing: Kirigami.Units.largeSpacing
                    QQC2.Switch {
                        id: sw
                        text: qsTr("Check in the background")
                        checked: page.scheduled
                        onToggled: {
                            scheduleCard.busy = true
                            if (checked) page.backend.start("scheduleInstall", "", interval.keys[interval.currentIndex], "", "")
                            else page.backend.start("scheduleRemove", "", "", "", "")
                        }
                    }
                    QQC2.ComboBox {
                        id: interval
                        readonly property var keys: ["hourly", "daily", "weekly"]
                        model: [qsTr("Every hour"), qsTr("Every day"), qsTr("Every week")]
                        currentIndex: Math.max(0, keys.indexOf(page.scheduledInterval))
                        Accessible.name: qsTr("How often")
                        onActivated: if (page.scheduled) {
                            scheduleCard.busy = true
                            page.backend.start("scheduleInstall", "", keys[currentIndex], "", "")
                        }
                    }
                    QQC2.BusyIndicator {
                        visible: scheduleCard.busy
                        running: visible
                        Layout.preferredHeight: Kirigami.Units.iconSizes.smallMedium
                        Layout.preferredWidth: Kirigami.Units.iconSizes.smallMedium
                    }
                }
            }

            SectionCard {
                title: qsTr("While appOFholding runs")
                iconName: "notifications"
                subtitle: qsTr("With the tray icon on, appOFholding also checks every few hours itself and announces each new version once, for apps set to “Tell me”.")
                QQC2.CheckBox {
                    text: qsTr("Notify me when an app update is released")
                    enabled: page.trayOn
                    checked: page.desk.notifyUpdates === true
                    onToggled: page.saveDesk("notifyUpdates", checked)
                }
                RowLayout {
                    visible: !page.trayOn
                    Layout.fillWidth: true
                    QQC2.Label {
                        text: qsTr("This needs the tray icon, which is off.")
                        opacity: 0.7
                        wrapMode: Text.Wrap
                        Layout.fillWidth: true
                    }
                    QQC2.Button {
                        flat: true
                        text: qsTr("Tray settings")
                        icon.name: "go-next"
                        onClicked: tabs.currentIndex = 3
                    }
                }
            }

            SectionCard {
                title: qsTr("Going back")
                iconName: "edit-undo"
                subtitle: qsTr("Each update keeps the versions it replaces, so you can roll back if a new one misbehaves. Lowering the number deletes the extra ones now.")
                RowLayout {
                    spacing: Kirigami.Units.largeSpacing
                    QQC2.Label { text: qsTr("Previous versions to keep:") }
                    QQC2.SpinBox {
                        id: keepBox
                        from: 0
                        to: page.defs.maxKeepVersions || 10
                        value: page.defs.keepVersions !== undefined ? page.defs.keepVersions : 2
                        editable: true
                        Accessible.name: qsTr("Previous versions to keep")
                        // Saved when the user stops changing it, so stepping 5 → 1 doesn't prune twice.
                        onValueModified: keepTimer.restart()
                        Timer {
                            id: keepTimer
                            interval: 800
                            onTriggered: if (keepBox.value !== page.defs.keepVersions) page.saveDefault("keepVersions", keepBox.value)
                        }
                    }
                }
            }
        }

        // ================= App defaults
        ColumnLayout {
            visible: tabs.currentIndex === 2
            Layout.fillWidth: true
            spacing: Kirigami.Units.largeSpacing

            QQC2.Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                opacity: 0.7
                text: qsTr("What apps start with. Each app can still be changed on its own page, and these also apply to apps added from the command line.")
            }

            SectionCard {
                title: qsTr("When a new version is found")
                iconName: "preferences-system-time"
                subtitle: qsTr("For apps whose update source is found from now on.")
                RowLayout {
                    Layout.fillWidth: true
                    spacing: Kirigami.Units.smallSpacing
                    QQC2.ComboBox {
                        id: defPolicy
                        Layout.fillWidth: true
                        Layout.maximumWidth: Kirigami.Units.gridUnit * 20
                        model: page.policyLabels
                        currentIndex: Math.max(0, page.policyKeys.indexOf(page.defs.policy || "notify"))
                        onActivated: (i) => page.saveDefault("policy", page.policyKeys[i])
                        Accessible.name: qsTr("Default update policy")
                    }
                    QQC2.Button {
                        text: qsTr("Apply to all apps…")
                        icon.name: "edit-select-all"
                        enabled: page.withSource > 0
                        onClicked: applyAll.open()
                    }
                }
            }

            SectionCard {
                title: qsTr("Sandbox")
                iconName: "security-high"
                subtitle: qsTr("For apps you add from now on. Apps already added keep their own sandbox.")
                QQC2.ComboBox {
                    id: defSandbox
                    readonly property var keys: ["none", "standard", "strict"]
                    Layout.fillWidth: true
                    Layout.maximumWidth: Kirigami.Units.gridUnit * 20
                    model: [qsTr("Off"), qsTr("Standard"), qsTr("Strict")]
                    currentIndex: Math.max(0, keys.indexOf(page.defs.sandbox || "none"))
                    onActivated: (i) => page.saveDefault("sandbox", keys[i])
                    Accessible.name: qsTr("Default sandbox")
                }
                QQC2.Label {
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    opacity: 0.7
                    text: [
                        qsTr("New apps run like any other program."),
                        qsTr("New apps work normally but can't see your files."),
                        qsTr("New apps can't see your files, and have no network or sound.")
                    ][defSandbox.currentIndex]
                }
            }
        }

        // ================= Tray & startup
        ColumnLayout {
            visible: tabs.currentIndex === 3
            Layout.fillWidth: true
            spacing: Kirigami.Units.largeSpacing

            Kirigami.InlineMessage {
                Layout.fillWidth: true
                visible: !page.win.trayAvailable
                type: Kirigami.MessageType.Warning
                text: qsTr("This desktop has no system tray. On GNOME, install the AppIndicator extension to use the tray.")
            }

            SectionCard {
                title: qsTr("System tray")
                iconName: "preferences-desktop-notification"
                subtitle: qsTr("Keep appOFholding running in the background so it can tell you when an app has a new version.")
                QQC2.Switch {
                    text: qsTr("Show an icon in the system tray")
                    checked: page.trayOn
                    onToggled: page.saveDesk("tray", checked)
                }
                ColumnLayout {
                    Layout.leftMargin: Kirigami.Units.gridUnit
                    enabled: page.trayOn
                    spacing: 0
                    QQC2.CheckBox {
                        text: qsTr("Close to the tray (keep running when the window is closed)")
                        checked: page.desk.closeToTray === true
                        onToggled: page.saveDesk("closeToTray", checked)
                    }
                    // Wayland never tells a window it was minimised, so the choice would do nothing.
                    QQC2.CheckBox {
                        visible: !page.wayland
                        text: qsTr("Minimise to the tray")
                        checked: page.desk.minimiseToTray === true
                        onToggled: page.saveDesk("minimiseToTray", checked)
                    }
                }
            }

            SectionCard {
                title: qsTr("Startup")
                iconName: "system-run"
                QQC2.Switch {
                    text: qsTr("Launch when I log in")
                    checked: page.desk.autostart === true
                    onToggled: page.saveDesk("autostart", checked)
                }
                QQC2.CheckBox {
                    Layout.leftMargin: Kirigami.Units.gridUnit
                    text: qsTr("Start in the tray, without opening the window")
                    enabled: page.trayOn && page.desk.autostart === true
                    checked: page.desk.startInTray === true
                    onToggled: page.saveDesk("startInTray", checked)
                }
            }
        }

        // ================= About
        ColumnLayout {
            visible: tabs.currentIndex === 4
            Layout.fillWidth: true
            spacing: Kirigami.Units.largeSpacing

            SectionCard {
                title: "appOFholding"
                iconName: "help-about"
                QQC2.Label {
                    text: qsTr("Keeps your AppImages tidy: menu entries, updates from GitHub and other sources, launch options and an optional sandbox.")
                    wrapMode: Text.Wrap
                    Layout.fillWidth: true
                }
                QQC2.Label { text: qsTr("Licensed under the GPL, version 3 or later."); opacity: 0.7 }
            }

            SectionCard {
                id: versionCard
                title: qsTr("Version")
                iconName: "update-none"
                readonly property var info: page.win.selfInfo
                readonly property bool busy: page.win.progress["@selfUpdate"] !== undefined
                // A check asked for here: say how it went (Main keeps the result itself).
                property bool checking: false
                Connections {
                    target: page.backend
                    function onFinished(kind, id, ok, payload) {
                        if (kind !== "selfCheck" || !versionCard.checking) return
                        versionCard.checking = false
                        if (!ok) page.win.showPassiveNotification(payload, "long")
                        else if (!Util.parse(payload).available) page.win.showPassiveNotification(qsTr("appOFholding is up to date"))
                    }
                }
                QQC2.Label {
                    text: !versionCard.info ? qsTr("Checking…")
                        : versionCard.info.available ? qsTr("You have %1; %2 is available.").arg(versionCard.info.current).arg(versionCard.info.latest)
                        : qsTr("%1, up to date.").arg(versionCard.info.current)
                }
                Flow {
                    Layout.fillWidth: true
                    spacing: Kirigami.Units.smallSpacing
                    PrimaryButton {
                        visible: !!versionCard.info && versionCard.info.available
                        text: versionCard.info && versionCard.info.canInstall
                              ? (versionCard.busy ? qsTr("Updating…") : qsTr("Update now"))
                              : qsTr("How to update")
                        icon.name: "update-none"
                        enabled: !versionCard.busy
                        onClicked: page.win.selfUpdateAction()
                    }
                    QQC2.Button {
                        visible: !!versionCard.info && !!versionCard.info.url
                        text: qsTr("What's new")
                        icon.name: "help-about"
                        onClicked: Qt.openUrlExternally(versionCard.info.url)
                    }
                    QQC2.Button {
                        text: qsTr("Check for a new appOFholding")
                        icon.name: "view-refresh"
                        enabled: !versionCard.checking
                        onClicked: { versionCard.checking = true; page.backend.start("selfCheck", "", "force", "", "") }
                    }
                }
            }
        }
    }

    readonly property int withSource: win.apps.filter(function (a) { return a.hasSource }).length

    Kirigami.PromptDialog {
        id: applyAll
        title: qsTr("Change every app?")
        subtitle: qsTr("All %n app(s) with an update source will be set to “%1”.", "", page.withSource)
            .arg(page.policyLabels[defPolicy.currentIndex])
        standardButtons: Kirigami.Dialog.Cancel
        customFooterActions: [
            Kirigami.Action {
                text: qsTr("Change all")
                icon.name: "dialog-ok-apply"
                onTriggered: {
                    applyAll.close()
                    var r = Util.parse(page.backend.applyPolicyToAll(page.policyKeys[defPolicy.currentIndex]))
                    if (r.error) page.win.showPassiveNotification(r.error, "long")
                    else page.win.showPassiveNotification(qsTr("Changed %n app(s)", "", r.changed))
                    page.win.reload()
                }
            }
        ]
    }
}
