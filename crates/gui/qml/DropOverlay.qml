// Drop AppImages anywhere on the window.
import QtQuick
import QtQuick.Window
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import QtQuick.Dialogs
import Qt.labs.platform as Platform
import org.kde.kirigami as Kirigami
import io.github.appofholding

DropArea {
    /// The window (Main.qml): its state and what it does.
    property var host

    id: dropArea
    anchors.fill: parent
    z: 1000
    onEntered: (drag) => { if (!drag.hasUrls) drag.accepted = false }
    onDropped: (drop) => {
        if (!drop.hasUrls) return
        var urls = []
        for (var i = 0; i < drop.urls.length; i++) urls.push(drop.urls[i].toString())
        drop.acceptProposedAction()
        dropArea.host.addFiles(urls)
    }
    Rectangle {
        anchors.fill: parent
        anchors.margins: Kirigami.Units.smallSpacing
        visible: dropArea.containsDrag
        color: Qt.rgba(Kirigami.Theme.highlightColor.r, Kirigami.Theme.highlightColor.g, Kirigami.Theme.highlightColor.b, 0.15)
        border.color: Kirigami.Theme.highlightColor
        border.width: 2
        radius: Kirigami.Units.cornerRadius
        ColumnLayout {
            anchors.centerIn: parent
            spacing: Kirigami.Units.smallSpacing
            Kirigami.Icon {
                source: "document-import"
                Layout.alignment: Qt.AlignHCenter
                Layout.preferredWidth: Kirigami.Units.iconSizes.huge
                Layout.preferredHeight: Kirigami.Units.iconSizes.huge
            }
            Kirigami.Heading {
                level: 2
                text: qsTr("Drop to add to your library")
                Layout.alignment: Qt.AlignHCenter
            }
        }
    }
}
