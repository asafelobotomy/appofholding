import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// One app as a tile (the "Grid" layout). The grid sets its width.
Rectangle {
    id: card
    required property var app
    readonly property real downloading: applicationWindow().progress[app.id] !== undefined ? applicationWindow().progress[app.id] : -2
    readonly property bool busy: downloading > -2
    readonly property bool hasUpdate: app.status === "update-available"
    readonly property bool hasProblem: !!app.error
    readonly property color accent: hasUpdate ? Kirigami.Theme.positiveTextColor
        : hasProblem ? Kirigami.Theme.neutralTextColor
        : Kirigami.Theme.textColor

    implicitHeight: column.implicitHeight + Kirigami.Units.largeSpacing * 2
    radius: Kirigami.Units.cornerRadius
    color: Kirigami.ColorUtils.tintWithAlpha(Kirigami.Theme.backgroundColor, Kirigami.Theme.textColor, hover.hovered ? 0.1 : 0.05)
    border.width: 1
    border.color: (hasUpdate || hasProblem)
        ? Kirigami.ColorUtils.tintWithAlpha(Kirigami.Theme.backgroundColor, accent, 0.45)
        : Kirigami.ColorUtils.tintWithAlpha(Kirigami.Theme.backgroundColor, Kirigami.Theme.textColor, 0.12)
    Behavior on color { ColorAnimation { duration: Kirigami.Units.shortDuration } }

    activeFocusOnTab: true
    Accessible.role: Accessible.Button
    Accessible.name: app.name
    Keys.onReturnPressed: applicationWindow().openApp(app.id)
    Keys.onEnterPressed: applicationWindow().openApp(app.id)

    HoverHandler { id: hover; cursorShape: Qt.PointingHandCursor }
    TapHandler { onTapped: applicationWindow().openApp(card.app.id) }

    Rectangle {
        anchors.fill: parent
        anchors.margins: -2
        radius: parent.radius + 2
        color: "transparent"
        border.width: 2
        border.color: Kirigami.Theme.highlightColor
        visible: card.activeFocus
    }

    // The overflow menu sits in the corner so the centred content stays centred.
    AppMenuButton {
        anchors { top: parent.top; right: parent.right; margins: Kirigami.Units.smallSpacing }
        app: card.app
        busy: card.busy
    }

    ColumnLayout {
        id: column
        anchors {
            left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter
            margins: Kirigami.Units.largeSpacing
        }
        spacing: Kirigami.Units.smallSpacing

        AppTile {
            source: card.app.icon
            size: Kirigami.Units.iconSizes.huge * 1.4
            Layout.alignment: Qt.AlignHCenter
        }

        RowLayout {
            Layout.alignment: Qt.AlignHCenter
            Layout.maximumWidth: column.width
            spacing: Kirigami.Units.smallSpacing
            Kirigami.Heading {
                text: card.app.name
                level: 4
                font.weight: Font.DemiBold
                elide: Text.ElideRight
                horizontalAlignment: Text.AlignHCenter
                Layout.fillWidth: true
            }
            Kirigami.Icon {
                visible: card.app.sandbox !== "none"
                source: "security-high"
                isMask: true
                color: card.app.sandbox === "strict" ? Kirigami.Theme.positiveTextColor : Kirigami.Theme.highlightColor
                Layout.preferredWidth: Kirigami.Units.iconSizes.small
                Layout.preferredHeight: Kirigami.Units.iconSizes.small
            }
        }
        Text {
            text: card.app.version || qsTr("version unknown")
            color: Kirigami.Theme.textColor
            opacity: 0.65
            font: Kirigami.Theme.smallFont
            elide: Text.ElideRight
            horizontalAlignment: Text.AlignHCenter
            Layout.fillWidth: true
        }

        Text {
            visible: !card.busy
            text: card.hasUpdate ? qsTr("Update available: %1").arg(card.app.latest)
                : card.hasProblem ? qsTr("Couldn't check for updates")
                : card.app.status === "up-to-date" ? qsTr("Up to date")
                : card.app.hasSource ? qsTr("Not checked yet") : qsTr("Updates are not set up")
            color: card.hasUpdate ? Kirigami.Theme.positiveTextColor : card.hasProblem ? Kirigami.Theme.neutralTextColor : Kirigami.Theme.textColor
            opacity: card.hasUpdate || card.hasProblem ? 1 : 0.8
            font.pointSize: Kirigami.Theme.defaultFont.pointSize * 0.92
            font.weight: card.hasUpdate ? Font.DemiBold : Font.Normal
            elide: Text.ElideRight
            horizontalAlignment: Text.AlignHCenter
            Layout.fillWidth: true
            HoverHandler { id: statusHover }
            QQC2.ToolTip.visible: statusHover.hovered && card.hasProblem
            QQC2.ToolTip.text: card.app.error || ""
        }
        ColumnLayout {
            visible: card.busy
            Layout.fillWidth: true
            spacing: 2
            QQC2.ProgressBar {
                indeterminate: card.downloading < 0
                value: Math.max(0, card.downloading)
                Layout.fillWidth: true
            }
            Text {
                text: card.downloading >= 0 ? qsTr("Updating… %1%").arg(Math.round(card.downloading * 100)) : qsTr("Working…")
                color: Kirigami.Theme.textColor
                opacity: 0.8
                font: Kirigami.Theme.smallFont
                horizontalAlignment: Text.AlignHCenter
                Layout.fillWidth: true
            }
        }

        RowLayout {
            Layout.alignment: Qt.AlignHCenter
            spacing: Kirigami.Units.smallSpacing
            PrimaryButton {
                visible: card.hasUpdate && !card.busy
                text: qsTr("Update")
                icon.name: "software-update-available"
                Accessible.name: qsTr("Update %1").arg(card.app.name)
                onClicked: applicationWindow().backendObject.start("update", card.app.id, "", "", "")
            }
            QQC2.Button {
                text: qsTr("Open")
                icon.name: "media-playback-start"
                flat: card.hasUpdate
                Accessible.name: qsTr("Open %1").arg(card.app.name)
                onClicked: {
                    var err = applicationWindow().backendObject.launch(card.app.id)
                    if (err) applicationWindow().showPassiveNotification(err)
                }
            }
        }
    }
}
