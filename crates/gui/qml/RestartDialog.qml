// Offered once appOFholding has updated itself: restart into the new version.
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
                if (err) restartDialog.host.showPassiveNotification(err, "long")
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
