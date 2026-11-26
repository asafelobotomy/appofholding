import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// Settings, in tabs: General, Updates, Defaults, Tray, About. Every control saves
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
    function openApplyAll() {
        applyAll.open()
    }

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

    // Short tab names: Settings often shares the window with the app list, and a long label
    // would be cut off.
    header: QQC2.TabBar {
        id: tabs
        QQC2.TabButton { text: qsTr("General"); icon.name: "configure" }
        QQC2.TabButton { text: qsTr("Updates"); icon.name: "view-refresh" }
        QQC2.TabButton { text: qsTr("Defaults"); icon.name: "view-list-icons" }
        QQC2.TabButton { text: qsTr("Tray"); icon.name: "preferences-desktop" }
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
        UpdatesTab {
            host: page
        }

        // ================= Defaults
        DefaultsTab {
            id: defaultsTab
            host: page
        }

        // ================= Tray (and startup)
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
                    Layout.fillWidth: true
                    enabled: page.trayOn
                    spacing: 0
                    Check {
                        text: qsTr("Close to the tray")
                        checked: page.desk.closeToTray === true
                        onToggled: page.saveDesk("closeToTray", checked)
                    }
                    QQC2.Label {
                        text: qsTr("Keeps running when the window is closed.")
                        wrapMode: Text.Wrap
                        opacity: 0.7
                        font: Kirigami.Theme.smallFont
                        Layout.fillWidth: true
                        Layout.leftMargin: Kirigami.Units.gridUnit * 1.5
                    }
                    // Wayland never tells a window it was minimised, so the choice would do nothing.
                    Check {
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
                Check {
                    Layout.leftMargin: Kirigami.Units.gridUnit
                    text: qsTr("Start hidden in the tray")
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
            .arg(page.policyLabels[defaultsTab.policyIndex])
        standardButtons: Kirigami.Dialog.Cancel
        customFooterActions: [
            Kirigami.Action {
                text: qsTr("Change all")
                icon.name: "dialog-ok-apply"
                onTriggered: {
                    applyAll.close()
                    var r = Util.parse(page.backend.applyPolicyToAll(page.policyKeys[defaultsTab.policyIndex]))
                    if (r.error) page.win.showPassiveNotification(r.error, "long")
                    else page.win.showPassiveNotification(qsTr("Changed %n app(s)", "", r.changed))
                    page.win.reload()
                }
            }
        ]
    }
}
