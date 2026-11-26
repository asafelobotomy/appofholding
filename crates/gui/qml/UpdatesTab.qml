// Settings › Updates: checking on a schedule and the update policy.
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

ColumnLayout {
    id: tab

    /// The settings page (SettingsPage.qml): its state and what it does.
    property var host

    visible: tab.host.tabIndex === 1
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
            target: tab.host.backend
            function onFinished(kind, id, ok, payload) {
                if (kind !== "scheduleInstall" && kind !== "scheduleRemove") return
                scheduleCard.busy = false
                if (!ok) tab.host.win.showPassiveNotification(payload, "long")
                tab.host.load()
                // A toggle or pick replaced the bindings; follow the real state again.
                sw.checked = Qt.binding(() => tab.host.scheduled)
                interval.currentIndex = Qt.binding(() => Math.max(0, interval.keys.indexOf(tab.host.scheduledInterval)))
            }
        }

        RowLayout {
            enabled: !scheduleCard.busy
            spacing: Kirigami.Units.largeSpacing
            QQC2.Switch {
                id: sw
                text: qsTr("Check in the background")
                checked: tab.host.scheduled
                onToggled: {
                    scheduleCard.busy = true
                    if (checked) tab.host.backend.start("scheduleInstall", "", interval.keys[interval.currentIndex], "", "")
                    else tab.host.backend.start("scheduleRemove", "", "", "", "")
                }
            }
            QQC2.ComboBox {
                id: interval
                readonly property var keys: ["hourly", "daily", "weekly"]
                model: [qsTr("Every hour"), qsTr("Every day"), qsTr("Every week")]
                currentIndex: Math.max(0, keys.indexOf(tab.host.scheduledInterval))
                Accessible.name: qsTr("How often")
                onActivated: if (tab.host.scheduled) {
                    scheduleCard.busy = true
                    tab.host.backend.start("scheduleInstall", "", keys[currentIndex], "", "")
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
        Check {
            text: qsTr("Notify me about new app versions")
            enabled: tab.host.trayOn
            checked: tab.host.desk.notifyUpdates === true
            onToggled: tab.host.saveDesk("notifyUpdates", checked)
        }
        RowLayout {
            visible: !tab.host.trayOn
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
                onClicked: tab.host.tabIndex = 3
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
                to: tab.host.defs.maxKeepVersions || 10
                value: tab.host.defs.keepVersions !== undefined ? tab.host.defs.keepVersions : 2
                editable: true
                Accessible.name: qsTr("Previous versions to keep")
                // Saved when the user stops changing it, so stepping 5 → 1 doesn't prune twice.
                onValueModified: keepTimer.restart()
                Timer {
                    id: keepTimer
                    interval: 800
                    onTriggered: if (keepBox.value !== tab.host.defs.keepVersions) tab.host.saveDefault("keepVersions", keepBox.value)
                }
            }
        }
    }
}
