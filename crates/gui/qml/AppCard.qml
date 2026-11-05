import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Kirigami.AbstractCard {
    id: card
    required property var modelData
    readonly property var app: modelData
    readonly property real downloading: applicationWindow().progress[app.id] !== undefined ? applicationWindow().progress[app.id] : -2

    showClickFeedback: true
    onClicked: applicationWindow().openApp(app.id)

    contentItem: RowLayout {
        spacing: Kirigami.Units.largeSpacing

        Kirigami.Icon {
            source: card.app.icon || "application-x-executable"
            fallback: "application-x-executable"
            Layout.preferredWidth: Kirigami.Units.iconSizes.huge
            Layout.preferredHeight: Kirigami.Units.iconSizes.huge
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 0

            Kirigami.Heading {
                text: card.app.name
                level: 3
                elide: Text.ElideRight
                Layout.fillWidth: true
            }
            QQC2.Label {
                text: card.app.version ? qsTr("Version %1").arg(card.app.version) : qsTr("Version unknown")
                opacity: 0.7
            }
            RowLayout {
                spacing: Kirigami.Units.smallSpacing
                Layout.topMargin: Kirigami.Units.smallSpacing

                Kirigami.Chip {
                    visible: card.app.status === "update-available"
                    text: qsTr("Update: %1").arg(card.app.latest)
                    icon.name: "update-none"
                    checkable: false
                    closable: false
                    onClicked: applicationWindow().openApp(card.app.id)
                }
                Kirigami.Chip {
                    visible: card.app.status === "up-to-date"
                    text: Util.statusLabel("up-to-date")
                    icon.name: "emblem-ok-symbolic"
                    checkable: false
                    closable: false
                    enabled: false
                }
                Kirigami.Chip {
                    visible: card.app.sandbox !== "none"
                    text: Util.sandboxLabel(card.app.sandbox)
                    icon.name: "security-high"
                    checkable: false
                    closable: false
                    enabled: false
                }
                Kirigami.Chip {
                    visible: !!card.app.error
                    text: qsTr("Last check failed")
                    icon.name: "dialog-warning"
                    checkable: false
                    closable: false
                    onClicked: applicationWindow().openApp(card.app.id)
                }
            }
            QQC2.ProgressBar {
                visible: card.downloading > -2
                indeterminate: card.downloading < 0
                value: Math.max(0, card.downloading)
                Layout.fillWidth: true
            }
        }

        QQC2.Button {
            text: qsTr("Open")
            icon.name: "media-playback-start"
            onClicked: {
                var err = applicationWindow().backendObject.launch(card.app.id)
                if (err) applicationWindow().showPassiveNotification(err)
            }
        }
    }
}
