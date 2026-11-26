// Where a dropped AppImage should live. Moving it into the library is the answer offered;
// copying or leaving it are a click away.
import QtQuick
import QtQuick.Window
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import QtQuick.Dialogs
import Qt.labs.platform as Platform
import org.kde.kirigami as Kirigami
import io.github.appofholding

Kirigami.Dialog {
    /// The window (Main.qml): its state and what it does.
    property var host

    id: placeDialog
    property var files: []
    property bool more: false
    readonly property string libDir: placeDialog.host.dataDir
    readonly property string mode: moveChoice.checked ? "move" : copyChoice.checked ? "copy" : "in-place"
    title: placeDialog.files.length === 1
        ? qsTr("Add %1").arg(placeDialog.files[0].name)
        : qsTr("Add %n AppImage(s)", "", placeDialog.files.length)
    padding: Kirigami.Units.largeSpacing
    preferredWidth: Kirigami.Units.gridUnit * 26
    standardButtons: Kirigami.Dialog.Cancel
    customFooterActions: [
        Kirigami.Action {
            text: qsTr("Always move")
            icon.name: "emblem-favorite"
            tooltip: qsTr("Move this and every AppImage you add from now on (change it in Settings)")
            onTriggered: {
                placeDialog.host.rememberDropAction("move")
                placeDialog.host.addWith(placeDialog.files, "move")
                placeDialog.close()
            }
        },
        Kirigami.Action {
            text: placeDialog.mode === "move" ? qsTr("Move")
                : placeDialog.mode === "copy" ? qsTr("Copy") : qsTr("Add where it is")
            icon.name: placeDialog.mode === "in-place" ? "list-add" : "document-import"
            onTriggered: {
                if (rememberChoice.checked) placeDialog.host.rememberDropAction(placeDialog.mode)
                placeDialog.host.addWith(placeDialog.files, placeDialog.mode)
                placeDialog.close()
            }
        }
    ]
    onOpened: { moveChoice.checked = true; rememberChoice.checked = false; more = false }

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
            text: placeDialog.mode === "in-place"
                ? qsTr("appOFholding adds it where it is and replaces it there when an update comes.")
                : (placeDialog.mode === "move"
                    ? qsTr("appOFholding moves it to %1, keeps it there with your other apps, and replaces it when an update comes.")
                    : qsTr("appOFholding copies it to %1, keeps the copy with your other apps, and replaces it when an update comes."))
                  .arg(placeDialog.libDir)
        }
        QQC2.Button {
            flat: true
            text: placeDialog.more ? qsTr("Fewer choices") : qsTr("Other choices…")
            icon.name: placeDialog.more ? "arrow-up" : "arrow-down"
            onClicked: placeDialog.more = !placeDialog.more
        }
        ColumnLayout {
            visible: placeDialog.more
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing
            Radio {
                id: moveChoice
                checked: true
                text: qsTr("Move it (recommended)")
            }
            Radio {
                id: copyChoice
                text: qsTr("Copy it and keep the original")
            }
            Radio {
                id: keepChoice
                text: qsTr("Leave it where it is")
            }
            QQC2.Label {
                visible: keepChoice.checked
                Layout.fillWidth: true
                Layout.leftMargin: Kirigami.Units.gridUnit * 1.5
                wrapMode: Text.Wrap
                opacity: 0.7
                font: Kirigami.Theme.smallFont
                text: qsTr("It stays your file: removing the app from appOFholding leaves it in place. Moving or renaming it later breaks the menu entry.")
            }
            Check {
                id: rememberChoice
                Layout.topMargin: Kirigami.Units.smallSpacing
                text: qsTr("Do this every time (change it in Settings)")
            }
        }
    }
}
