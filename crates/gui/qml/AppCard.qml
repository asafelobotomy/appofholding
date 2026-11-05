import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// One app in the list: icon, name, a one-line status that says what matters, and the action
// you most likely want (Update when there is one, otherwise Open).
Rectangle {
    id: card
    required property var modelData
    readonly property var app: modelData
    readonly property real downloading: applicationWindow().progress[app.id] !== undefined ? applicationWindow().progress[app.id] : -2
    readonly property bool busy: downloading > -2
    readonly property bool hasUpdate: app.status === "update-available"
    readonly property bool hasProblem: !!app.error
    // In a narrow list the buttons shrink to icons so the text keeps its room.
    readonly property bool compact: width < Kirigami.Units.gridUnit * 30
    readonly property color accent: hasUpdate ? Kirigami.Theme.positiveTextColor
        : hasProblem ? Kirigami.Theme.neutralTextColor
        : Kirigami.Theme.textColor

    width: ListView.view ? ListView.view.width - ListView.view.leftMargin - ListView.view.rightMargin : implicitWidth
    implicitHeight: row.implicitHeight + Kirigami.Units.largeSpacing * 2
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

    // Keyboard focus ring
    Rectangle {
        anchors.fill: parent
        anchors.margins: -2
        radius: parent.radius + 2
        color: "transparent"
        border.width: 2
        border.color: Kirigami.Theme.highlightColor
        visible: card.activeFocus
    }

    RowLayout {
        id: row
        anchors {
            left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter
            margins: Kirigami.Units.largeSpacing
        }
        spacing: Kirigami.Units.largeSpacing

        AppTile { source: card.app.icon }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 2

            RowLayout {
                spacing: Kirigami.Units.smallSpacing
                Kirigami.Heading {
                    text: card.app.name
                    level: 4
                    font.weight: Font.DemiBold
                    elide: Text.ElideRight
                    Layout.maximumWidth: card.width * 0.45
                }
                Kirigami.Icon {
                    visible: card.app.sandbox !== "none"
                    source: "security-high"
                    isMask: true
                    color: card.app.sandbox === "strict" ? Kirigami.Theme.positiveTextColor : Kirigami.Theme.highlightColor
                    Layout.preferredWidth: Kirigami.Units.iconSizes.small
                    Layout.preferredHeight: Kirigami.Units.iconSizes.small
                    HoverHandler { id: shieldHover }
                    QQC2.ToolTip.visible: shieldHover.hovered
                    QQC2.ToolTip.text: Util.sandboxLabel(card.app.sandbox)
                }
            }

            Text {
                text: (card.app.version ? card.app.version : qsTr("version unknown")) + "  ·  " + (card.app.source ? card.app.source.replace(/ \[.*\]$/, "") : qsTr("no update source"))
                color: Kirigami.Theme.textColor
                opacity: 0.65
                font: Kirigami.Theme.smallFont
                elide: Text.ElideRight
                Layout.fillWidth: true
            }

            RowLayout {
                spacing: Kirigami.Units.smallSpacing
                visible: !card.busy
                Rectangle {
                    width: 8; height: 8; radius: 4
                    color: card.hasUpdate ? Kirigami.Theme.positiveTextColor
                        : card.hasProblem ? Kirigami.Theme.neutralTextColor
                        : card.app.status === "up-to-date" ? Kirigami.ColorUtils.tintWithAlpha(Kirigami.Theme.backgroundColor, Kirigami.Theme.positiveTextColor, 0.7)
                        : Kirigami.Theme.disabledTextColor
                }
                Text {
                    text: card.hasUpdate ? qsTr("Update available: %1").arg(card.app.latest)
                        : card.hasProblem ? qsTr("Couldn't check for updates")
                        : card.app.status === "up-to-date" ? qsTr("Up to date")
                        : card.app.hasSource ? qsTr("Not checked yet") : qsTr("Updates are not set up")
                    color: card.hasUpdate ? Kirigami.Theme.positiveTextColor : card.hasProblem ? Kirigami.Theme.neutralTextColor : Kirigami.Theme.textColor
                    opacity: card.hasUpdate || card.hasProblem ? 1 : 0.8
                    font.pointSize: Kirigami.Theme.defaultFont.pointSize * 0.92
                    font.weight: card.hasUpdate ? Font.DemiBold : Font.Normal
                    elide: Text.ElideRight
                    Layout.fillWidth: true
                    HoverHandler { id: statusHover }
                    QQC2.ToolTip.visible: statusHover.hovered && card.hasProblem
                    QQC2.ToolTip.text: card.app.error || ""
                }
            }
            RowLayout {
                visible: card.busy
                spacing: Kirigami.Units.smallSpacing
                QQC2.ProgressBar {
                    indeterminate: card.downloading < 0
                    value: Math.max(0, card.downloading)
                    Layout.fillWidth: true
                    Layout.maximumWidth: Kirigami.Units.gridUnit * 14
                }
                Text {
                    text: card.downloading >= 0 ? qsTr("Updating… %1%").arg(Math.round(card.downloading * 100)) : qsTr("Working…")
                    color: Kirigami.Theme.textColor
                    opacity: 0.8
                    font: Kirigami.Theme.smallFont
                }
            }
        }

        PrimaryButton {
            visible: card.hasUpdate && !card.busy
            text: qsTr("Update")
            icon.name: "software-update-available"
            display: card.compact ? QQC2.AbstractButton.IconOnly : QQC2.AbstractButton.TextBesideIcon
            // Some styles size an icon-only button for its hidden text; keep it square.
            Layout.preferredWidth: card.compact ? implicitHeight : -1
            QQC2.ToolTip.visible: hovered && card.compact
            QQC2.ToolTip.text: qsTr("Update to %1").arg(card.app.latest)
            Accessible.name: qsTr("Update %1").arg(card.app.name)
            onClicked: applicationWindow().backendObject.start("update", card.app.id, "", "", "")
        }
        QQC2.Button {
            text: qsTr("Open")
            icon.name: "media-playback-start"
            flat: card.hasUpdate
            display: card.compact ? QQC2.AbstractButton.IconOnly : QQC2.AbstractButton.TextBesideIcon
            Layout.preferredWidth: card.compact ? implicitHeight : -1
            QQC2.ToolTip.visible: hovered && card.compact
            QQC2.ToolTip.text: qsTr("Open %1").arg(card.app.name)
            Accessible.name: qsTr("Open %1").arg(card.app.name)
            onClicked: {
                var err = applicationWindow().backendObject.launch(card.app.id)
                if (err) applicationWindow().showPassiveNotification(err)
            }
        }
        AppMenuButton { app: card.app; busy: card.busy }
    }
}
