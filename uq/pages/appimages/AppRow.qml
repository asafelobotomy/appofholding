// One AppImage: its icon, name and how it is doing, and what can be done with it: Update, its
// update policy, and a menu with Skip this version, Remove and Remove and delete its data.
// Removal asks first and shows everything it would remove (uQ's confirmation sheet).
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings
import "logic.js" as Logic

ColumnLayout {
    id: row

    property var host
    property var app
    /// Show the policy and the menu, or only the Update button (the Overview).
    property bool full: true
    readonly property string key: "app:" + row.app.id
    readonly property var status: Logic.statusOf(row.app)
    readonly property bool idle: !!row.host && !row.host.busy

    spacing: 4

    RowLayout {
        spacing: 12
        Layout.fillWidth: true

        UqAppIcon {
            name: row.app.icon
            label: row.app.name
            size: 32
        }
        ColumnLayout {
            spacing: 0
            Layout.fillWidth: true

            UqLabel {
                text: row.app.name
                elide: Text.ElideRight
                Layout.fillWidth: true
            }
            UqLabel {
                id: subtitle
                text: Logic.subtitleOf(row.app) + (Logic.sizeText(row.app.sizeBytes) !== "" && row.full ? " · " + Logic.sizeText(row.app.sizeBytes) : "")
                elide: Text.ElideRight
                opacity: row.status.tone === "plain" ? 0.7 : 1
                Layout.fillWidth: true
            }
            UqLabel {
                id: scanNote
                objectName: "scan-note-" + row.app.id
                text: Logic.scanText(row.app)
                visible: row.full && text !== ""
                wrapMode: Text.Wrap
                opacity: row.app.scan && row.app.scan.state === "threat" ? 1 : 0.7
                Layout.fillWidth: true
            }
            Binding {
                target: scanNote
                property: "color"
                value: row.host.errorColor()
                when: !!row.host && !!row.app.scan && row.app.scan.state === "threat"
            }
            Binding {
                target: subtitle
                property: "color"
                value: row.status.tone === "problem" ? row.host.errorColor() : row.host.accentColor()
                when: row.status.tone !== "plain" && !!row.host
            }
        }
        QQC2.Button {
            text: "Update"
            icon.name: "system-software-update"
            visible: row.app.update
            enabled: row.idle
            onClicked: row.host.stream("update", "update", [row.app.id], row.key)
        }
        UqComboBox {
            id: policy
            visible: row.full && row.app.hasSource
            enabled: row.idle
            model: Logic.POLICY_NAMES
            currentIndex: Logic.indexOfKey(Logic.POLICY_KEYS, row.app.policy, 1)
            implicitWidth: 230
            onActivated: index => {
                // Show what is really set until the answer comes.
                policy.currentIndex = Qt.binding(() => Logic.indexOfKey(Logic.POLICY_KEYS, row.app.policy, 1));
                row.host.write(row.key, "policy", [row.app.id, Logic.POLICY_KEYS[index]]);
            }
        }
        QQC2.Button {
            id: more
            visible: row.full
            text: "More"
            icon.name: "overflow-menu"
            enabled: row.idle
            onClicked: menu.open()

            QQC2.Menu {
                id: menu
                y: more.height

                QQC2.MenuItem {
                    text: "Skip this version"
                    visible: row.app.update
                    height: visible ? implicitHeight : 0
                    onTriggered: row.host.write(row.key, "skip", [row.app.id])
                }
                QQC2.MenuItem {
                    text: "Stop skipping " + row.app.skipped
                    visible: !row.app.update && row.app.skipped !== ""
                    height: visible ? implicitHeight : 0
                    onTriggered: row.host.write(row.key, "unskip", [row.app.id])
                }
                QQC2.MenuItem {
                    objectName: "scan-" + row.app.id
                    text: "Scan with oysterAV"
                    onTriggered: row.host.stream("scan", "scan", [row.app.id], row.key)
                }
                QQC2.MenuItem {
                    text: "Remove…"
                    onTriggered: row.host.write(row.key, "remove", [row.app.id], null, "section")
                }
                QQC2.MenuItem {
                    text: "Remove and delete its data…"
                    onTriggered: row.host.write(row.key, "remove-data", [row.app.id], null, "section")
                }
            }
        }
    }
    ProgressView {
        host: row.host
        progressKey: row.key
        Layout.fillWidth: true
    }
    Outcome {
        host: row.host
        noteKey: row.key
        Layout.fillWidth: true
    }
}
