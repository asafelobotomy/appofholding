import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Kirigami.ScrollablePage {
    id: page
    title: qsTr("Settings")
    readonly property var backend: applicationWindow().backendObject
    property bool scheduled: false

    function load() { scheduled = !!Util.parse(backend.scheduleStatus()).installed }
    Component.onCompleted: load()

    ColumnLayout {
        spacing: Kirigami.Units.largeSpacing

        Section {
            first: true
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
                    currentIndex: 1
                    onActivated: if (page.scheduled) {
                        var err = page.backend.scheduleInstall(keys[currentIndex])
                        if (err) applicationWindow().showPassiveNotification(err)
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
        }
    }
}
