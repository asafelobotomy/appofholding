// A newer version of an app we have: offer to update that app with it, one file at a time.
import QtQuick
import QtQuick.Window
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import QtQuick.Dialogs
import Qt.labs.platform as Platform
import org.kde.kirigami as Kirigami
import io.github.appofholding

Kirigami.PromptDialog {
    /// The window (Main.qml): its state and what it does.
    property var host

    id: updateDialog
    property var queue: []
    readonly property var file: queue.length > 0 ? queue[0] : null
    readonly property var known: file ? file.known : null
    readonly property bool newer: !!known && known.relation === "newer"

    function offer(files) {
        queue = queue.concat(files)
        if (!visible) open()
    }
    function next() {
        queue = queue.slice(1)
        if (queue.length > 0) keepOriginal.checked = updateDialog.host.desk.dropAction === "copy" || updateDialog.host.desk.dropAction === "in-place"
        else close()
    }
    onOpened: keepOriginal.checked = updateDialog.host.desk.dropAction === "copy" || updateDialog.host.desk.dropAction === "in-place"
    onRejected: queue = []

    title: !known ? ""
        : newer ? qsTr("Update %1 to %2?").arg(known.name).arg(known.version)
        : qsTr("Replace %1 with this file?").arg(known.name)
    subtitle: !known ? ""
        : (newer ? qsTr("%1 is a newer version than the one you have (%2).").arg(file.name).arg(known.installed || "?")
                 : qsTr("%1 is another build of %2; its version (%3) can't be compared with yours (%4).")
                       .arg(file.name).arg(known.name).arg(known.version || "?").arg(known.installed || "?"))
          + " " + qsTr("The installed version is kept so you can roll back.")
    standardButtons: Kirigami.Dialog.Cancel
    customFooterActions: [
        Kirigami.Action {
            text: updateDialog.newer ? qsTr("Update") : qsTr("Replace")
            icon.name: "software-update-available"
            onTriggered: {
                var f = updateDialog.file
                backend.start("updateFromFile", f.known.id, f.path, keepOriginal.checked ? "copy" : "move", "")
                updateDialog.next()
            }
        },
        Kirigami.Action {
            text: qsTr("Not this one")
            icon.name: "go-next"
            visible: updateDialog.queue.length > 1
            onTriggered: updateDialog.next()
        }
    ]
    Check {
        id: keepOriginal
        text: qsTr("Keep the original file (install a copy)")
    }
}
