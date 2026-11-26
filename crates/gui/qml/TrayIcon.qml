// The tray icon and its menu.
import QtQuick
import QtQuick.Window
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import QtQuick.Dialogs
import Qt.labs.platform as Platform
import org.kde.kirigami as Kirigami
import io.github.appofholding

Platform.SystemTrayIcon {
    /// The window (Main.qml): its state and what it does.
    property var host

    id: tray
    visible: tray.host.desk.tray === true
    icon.name: "io.github.appofholding"
    icon.source: "qrc:/appofholding/app-icon.svg"
    tooltip: tray.host.updateCount > 0 ? qsTr("appOFholding — %n update(s) available", "", tray.host.updateCount) : "appOFholding"

    onActivated: (reason) => {
        if (reason === Platform.SystemTrayIcon.Context) return
        if (tray.host.visible && tray.host.active) tray.host.hideToTray()
        else tray.host.showWindow()
    }
    onMessageClicked: {
        tray.host.showWindow()
        if (tray.host.announcedApp) tray.host.openApp(tray.host.announcedApp)
    }

    menu: Platform.Menu {
        Platform.MenuItem {
            text: tray.host.visible ? qsTr("Hide appOFholding") : qsTr("Show appOFholding")
            onTriggered: tray.host.visible ? tray.host.hideToTray() : tray.host.showWindow()
        }
        Platform.MenuItem {
            text: qsTr("Check for updates")
            enabled: !tray.host.working
            onTriggered: tray.host.checkAll()
        }
        Platform.MenuSeparator {}
        Platform.MenuItem {
            text: qsTr("Quit")
            onTriggered: Qt.quit()
        }
    }
}
