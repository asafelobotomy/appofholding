import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

ThemedPage {
    id: page
    title: qsTr("Settings")
    readonly property var backend: applicationWindow().backendObject
    property bool scheduled: false
    property string scheduledInterval: "daily"

    function load() {
        var st = Util.parse(backend.scheduleStatus())
        scheduled = !!st.installed
        if (st.interval) scheduledInterval = st.interval
    }
    Component.onCompleted: load()

    ColumnLayout {
        spacing: Kirigami.Units.largeSpacing

        Section {
            first: true
            title: qsTr("Appearance")
            subtitle: qsTr("Pick a colour theme. Changes apply the next time appOFholding starts.")
            QQC2.ComboBox {
                id: themeBox
                Layout.fillWidth: true
                model: Themes.labels
                currentIndex: Math.max(0, Themes.names.indexOf(applicationWindow().themeName))
                onActivated: {
                    var err = page.backend.setTheme(Themes.names[currentIndex])
                    if (err) applicationWindow().showPassiveNotification(err)
                    else applicationWindow().showPassiveNotification(qsTr("Restart appOFholding to see the new theme."))
                }
            }
        }

        Section {
            id: deskSection
            title: qsTr("System tray and startup")
            subtitle: qsTr("Keep appOFholding running in the background so it can tell you when an app has a new version.")
            readonly property var desk: applicationWindow().desk
            readonly property bool trayOn: desk.tray === true
            readonly property bool wayland: Qt.platform.pluginName.indexOf("wayland") >= 0

            function save(key, value) {
                var d = Object.assign({}, applicationWindow().desk)
                delete d.error
                d[key] = value
                var err = page.backend.setDesktopSettings(JSON.stringify(d))
                if (err) applicationWindow().showPassiveNotification(err, "long")
                applicationWindow().reloadDesktop()
            }

            Kirigami.InlineMessage {
                Layout.fillWidth: true
                visible: !applicationWindow().trayAvailable
                type: Kirigami.MessageType.Warning
                text: qsTr("This desktop has no system tray. On GNOME, install the AppIndicator extension to use these options.")
            }
            QQC2.Switch {
                text: qsTr("Show an icon in the system tray")
                checked: deskSection.trayOn
                onToggled: deskSection.save("tray", checked)
            }
            ColumnLayout {
                Layout.leftMargin: Kirigami.Units.gridUnit
                enabled: deskSection.trayOn
                spacing: 0
                QQC2.CheckBox {
                    text: qsTr("Minimise to the tray")
                    enabled: !deskSection.wayland
                    checked: deskSection.desk.minimiseToTray === true
                    onToggled: deskSection.save("minimiseToTray", checked)
                }
                QQC2.Label {
                    visible: deskSection.wayland
                    text: qsTr("Wayland does not tell programs when they are minimised; use “close to the tray” instead.")
                    wrapMode: Text.Wrap
                    opacity: 0.7
                    font: Kirigami.Theme.smallFont
                    Layout.fillWidth: true
                    Layout.leftMargin: Kirigami.Units.gridUnit * 1.5
                }
                QQC2.CheckBox {
                    text: qsTr("Close to the tray (keep running when the window is closed)")
                    checked: deskSection.desk.closeToTray === true
                    onToggled: deskSection.save("closeToTray", checked)
                }
                QQC2.CheckBox {
                    text: qsTr("Notify me when an app update is released")
                    checked: deskSection.desk.notifyUpdates === true
                    onToggled: deskSection.save("notifyUpdates", checked)
                }
                QQC2.Label {
                    text: qsTr("Checks every few hours while appOFholding runs. Each new version is announced once, for apps set to “Tell me”.")
                    wrapMode: Text.Wrap
                    opacity: 0.7
                    font: Kirigami.Theme.smallFont
                    Layout.fillWidth: true
                    Layout.leftMargin: Kirigami.Units.gridUnit * 1.5
                }
            }
            QQC2.Switch {
                text: qsTr("Launch at startup")
                checked: deskSection.desk.autostart === true
                onToggled: deskSection.save("autostart", checked)
            }
            QQC2.CheckBox {
                Layout.leftMargin: Kirigami.Units.gridUnit
                text: qsTr("Start in the tray, without opening the window")
                enabled: deskSection.trayOn && deskSection.desk.autostart === true
                checked: deskSection.desk.startInTray === true
                onToggled: deskSection.save("startInTray", checked)
            }
        }

        Section {
            title: qsTr("Adding AppImages")
            subtitle: qsTr("What happens to an AppImage you drop on the window or open with appOFholding. Files already in %1 are always just added.").arg(page.backend.dataDir())
            QQC2.ComboBox {
                readonly property var keys: ["ask", "move", "copy", "in-place"]
                Layout.fillWidth: true
                Layout.maximumWidth: Kirigami.Units.gridUnit * 24
                model: [
                    qsTr("Ask each time"),
                    qsTr("Move it into the apps folder"),
                    qsTr("Copy it into the apps folder"),
                    qsTr("Leave it where it is")
                ]
                currentIndex: Math.max(0, keys.indexOf(deskSection.desk.dropAction))
                onActivated: (i) => deskSection.save("dropAction", keys[i])
                Accessible.name: qsTr("Adding AppImages")
            }
        }

        Section {
            title: qsTr("Automatic update checks")
            subtitle: qsTr("A background timer checks your apps for updates and installs the ones set to update automatically. Apps set to notify get one notification per new version.")

            RowLayout {
                QQC2.Switch {
                    id: sw
                    checked: page.scheduled
                    onToggled: {
                        var err = checked ? page.backend.scheduleInstall(interval.keys[interval.currentIndex]) : page.backend.scheduleRemove()
                        if (err) applicationWindow().showPassiveNotification(err)
                        page.load()
                    }
                }
                QQC2.Label { text: page.scheduled ? qsTr("On") : qsTr("Off") }
                QQC2.ComboBox {
                    id: interval
                    readonly property var keys: ["hourly", "daily", "weekly"]
                    model: [qsTr("Every hour"), qsTr("Every day"), qsTr("Every week")]
                    currentIndex: Math.max(0, keys.indexOf(page.scheduledInterval))
                    onActivated: if (page.scheduled) {
                        var err = page.backend.scheduleInstall(keys[currentIndex])
                        if (err) applicationWindow().showPassiveNotification(err)
                        page.load()
                    }
                }
            }
        }

        Section {
            title: qsTr("Where apps are kept")
            QQC2.Label { text: page.backend.dataDir(); font.family: "monospace"; wrapMode: Text.WrapAnywhere; Layout.fillWidth: true }
        }

        Section {
            title: qsTr("About")
            QQC2.Label {
                text: qsTr("appOFholding keeps your AppImages tidy: menu entries, updates from GitHub and other sources, launch options and an optional sandbox.")
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
            QQC2.Label { text: qsTr("Licensed under the GPL, version 3 or later."); opacity: 0.7 }
            RowLayout {
                QQC2.Label {
                    readonly property var info: applicationWindow().selfInfo
                    text: !info ? qsTr("Version: checking…")
                        : info.available ? qsTr("Version %1 — %2 is available").arg(info.current).arg(info.latest)
                        : qsTr("Version %1 — up to date").arg(info.current)
                }
                PrimaryButton {
                    readonly property var info: applicationWindow().selfInfo
                    visible: !!info && info.available && info.canInstall
                    text: qsTr("Update now")
                    icon.name: "update-none"
                    enabled: applicationWindow().progress["@selfUpdate"] === undefined
                    onClicked: applicationWindow().confirmSelfUpdate()
                }
                QQC2.Button {
                    text: qsTr("Check now")
                    icon.name: "view-refresh"
                    enabled: !applicationWindow().working
                    onClicked: page.backend.start("selfCheck", "", "force", "", "")
                }
            }
        }
    }
}
