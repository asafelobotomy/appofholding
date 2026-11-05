import QtQuick
import QtQuick.Controls as QQC2

// The "more actions" button shared by every way of listing apps.
QQC2.ToolButton {
    id: btn
    required property var app
    property bool busy: false

    icon.name: "overflow-menu"
    Accessible.name: qsTr("More actions for %1").arg(app.name)
    onClicked: menu.popup()

    QQC2.Menu {
        id: menu
        QQC2.MenuItem {
            text: qsTr("Details")
            icon.name: "configure"
            onTriggered: applicationWindow().openApp(btn.app.id)
        }
        QQC2.MenuItem {
            text: qsTr("Check for updates")
            icon.name: "view-refresh"
            enabled: btn.app.hasSource && !btn.busy
            onTriggered: applicationWindow().backendObject.start("check", btn.app.id, "", "", "")
        }
    }
}
