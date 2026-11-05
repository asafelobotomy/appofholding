import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Kirigami.ScrollablePage {
    id: page
    title: qsTr("Apps")

    actions: [
        Kirigami.Action {
            text: qsTr("Add app")
            icon.name: "list-add"
            Kirigami.Action {
                text: qsTr("From a link…")
                icon.name: "download"
                onTriggered: applicationWindow().openAddDialog()
            }
            Kirigami.Action {
                text: qsTr("From a file…")
                icon.name: "document-open"
                onTriggered: applicationWindow().openFileDialog()
            }
        },
        Kirigami.Action {
            text: qsTr("Check for updates")
            icon.name: "view-refresh"
            enabled: !applicationWindow().working
            onTriggered: applicationWindow().checkAll()
        }
    ]

    Kirigami.CardsListView {
        id: list
        model: applicationWindow().apps
        spacing: Kirigami.Units.smallSpacing
        delegate: AppCard {}

        Kirigami.PlaceholderMessage {
            anchors.centerIn: parent
            width: parent.width - Kirigami.Units.gridUnit * 4
            visible: list.count === 0
            icon.name: "application-x-executable"
            text: qsTr("No apps yet")
            explanation: qsTr("Drop an AppImage here, or add one from a file or a GitHub link.")
            helpfulAction: Kirigami.Action {
                text: qsTr("Add from a link…")
                icon.name: "download"
                onTriggered: applicationWindow().openAddDialog()
            }
        }
    }

    DropArea {
        anchors.fill: parent
        onDropped: (drop) => {
            for (var i = 0; i < drop.urls.length; i++) {
                applicationWindow().backendObject.start("addFile", "", drop.urls[i].toString(), "", "")
            }
        }
        Rectangle {
            anchors.fill: parent
            visible: parent.containsDrag
            color: Qt.rgba(Kirigami.Theme.highlightColor.r, Kirigami.Theme.highlightColor.g, Kirigami.Theme.highlightColor.b, 0.15)
            border.color: Kirigami.Theme.highlightColor
            border.width: 2
            radius: Kirigami.Units.smallSpacing
            QQC2.Label {
                anchors.centerIn: parent
                text: qsTr("Drop to add")
                font.pointSize: Kirigami.Theme.defaultFont.pointSize * 1.5
            }
        }
    }
}
