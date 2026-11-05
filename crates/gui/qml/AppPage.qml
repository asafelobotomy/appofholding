import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Kirigami.ScrollablePage {
    id: page

    property string appId
    property var d: ({})
    property string error
    readonly property var backend: applicationWindow().backendObject

    title: d.name || appId

    function load() {
        var r = Util.parse(backend.detail(appId))
        if (r.error) {
            error = r.error
            d = ({})
        } else {
            error = ""
            d = r
        }
    }

    Component.onCompleted: load()

    Connections {
        target: applicationWindow()
        function onRevisionChanged() { page.load() }
    }

    actions: [
        Kirigami.Action {
            text: qsTr("Open")
            icon.name: "media-playback-start"
            onTriggered: {
                var err = page.backend.launch(page.appId)
                if (err) applicationWindow().showPassiveNotification(err)
            }
        },
        Kirigami.Action {
            text: qsTr("Remove…")
            icon.name: "edit-delete"
            onTriggered: removeDialog.open()
        }
    ]

    ColumnLayout {
        spacing: Kirigami.Units.largeSpacing

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: page.error.length > 0
            type: Kirigami.MessageType.Error
            text: page.error
        }

        RowLayout {
            Layout.fillWidth: true
            visible: !!page.d.id
            spacing: Kirigami.Units.largeSpacing
            Kirigami.Icon {
                source: page.d.icon || "application-x-executable"
                fallback: "application-x-executable"
                Layout.preferredWidth: Kirigami.Units.iconSizes.enormous
                Layout.preferredHeight: Kirigami.Units.iconSizes.enormous
            }
            ColumnLayout {
                Layout.fillWidth: true
                Kirigami.Heading { text: page.d.name || ""; level: 1 }
                QQC2.Label { text: page.d.version ? qsTr("Version %1").arg(page.d.version) : qsTr("Version unknown") }
                QQC2.Label {
                    text: (page.d.path || "") + (page.d.size ? "  ·  " + Util.fmtSize(page.d.size) : "")
                    opacity: 0.7
                    elide: Text.ElideMiddle
                    Layout.fillWidth: true
                }
                QQC2.Label {
                    visible: !!page.d.running
                    text: qsTr("Running now")
                    color: Kirigami.Theme.positiveTextColor
                }
            }
        }

        SourceSection { detail: page.d; appId: page.appId; backend: page.backend; Layout.fillWidth: true }
        LaunchSection { detail: page.d; appId: page.appId; backend: page.backend; Layout.fillWidth: true }
        SandboxSection { detail: page.d; appId: page.appId; backend: page.backend; Layout.fillWidth: true }
    }

    QQC2.Dialog {
        id: removeDialog
        title: qsTr("Remove %1?").arg(page.d.name || page.appId)
        modal: true
        anchors.centerIn: QQC2.Overlay.overlay
        standardButtons: QQC2.Dialog.Yes | QQC2.Dialog.Cancel
        contentItem: ColumnLayout {
            spacing: Kirigami.Units.largeSpacing
            QQC2.Label {
                text: qsTr("This removes the menu entry and the AppImage file.")
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
            QQC2.CheckBox {
                id: purge
                text: qsTr("Also delete the app's private data (cannot be undone)")
                visible: page.d.sandbox && page.d.sandbox.level !== "none"
            }
        }
        onAccepted: {
            var err = page.backend.remove(page.appId, purge.checked)
            if (err) {
                applicationWindow().showPassiveNotification(err)
            } else {
                applicationWindow().reload()
                applicationWindow().pageStack.pop()
            }
        }
    }
}
