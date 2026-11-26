// Updating appOFholding itself: say exactly what will happen before doing it.
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

    id: selfUpdateDialog
    readonly property var info: selfUpdateDialog.host.selfInfo
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
