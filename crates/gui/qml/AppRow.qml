import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// One app as a single thin line (the "List (Compact)" layout).
Rectangle {
    id: row
    required property var modelData
    readonly property var app: modelData
    readonly property real downloading: applicationWindow().progress[app.id] !== undefined ? applicationWindow().progress[app.id] : -2
    readonly property bool busy: downloading > -2
    readonly property bool hasUpdate: app.status === "update-available"
    readonly property bool hasProblem: !!app.error
    // Below this width only the name, version and buttons are shown.
    readonly property bool narrow: width < Kirigami.Units.gridUnit * 32

    width: ListView.view ? ListView.view.width - ListView.view.leftMargin - ListView.view.rightMargin : implicitWidth
    implicitHeight: Kirigami.Units.iconSizes.medium + Kirigami.Units.smallSpacing * 2
    radius: Kirigami.Units.cornerRadius
    color: hover.hovered ? Kirigami.ColorUtils.tintWithAlpha(Kirigami.Theme.backgroundColor, Kirigami.Theme.textColor, 0.1) : "transparent"

    activeFocusOnTab: true
    Accessible.role: Accessible.Button
    Accessible.name: app.name
    Keys.onReturnPressed: applicationWindow().openApp(app.id)
    Keys.onEnterPressed: applicationWindow().openApp(app.id)

    HoverHandler { id: hover; cursorShape: Qt.PointingHandCursor }
    TapHandler { onTapped: applicationWindow().openApp(row.app.id) }

    Rectangle {
        anchors.fill: parent
        anchors.margins: -1
        radius: parent.radius + 1
        color: "transparent"
        border.width: 2
        border.color: Kirigami.Theme.highlightColor
        visible: row.activeFocus
    }

    RowLayout {
        anchors {
            left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter
            leftMargin: Kirigami.Units.smallSpacing * 2; rightMargin: Kirigami.Units.smallSpacing
        }
        spacing: Kirigami.Units.smallSpacing * 2

        Kirigami.Icon {
            source: row.app.icon || "application-x-executable"
            fallback: "application-x-executable"
            Layout.preferredWidth: Kirigami.Units.iconSizes.medium
            Layout.preferredHeight: Kirigami.Units.iconSizes.medium
        }
        Text {
            text: row.app.name
            color: Kirigami.Theme.textColor
            font.weight: Font.DemiBold
            elide: Text.ElideRight
            // A fixed share of the line, so versions line up from row to row.
            Layout.preferredWidth: row.width * 0.3
            Layout.maximumWidth: row.width * 0.3
        }
        // Always takes its place so the columns stay aligned.
        Item {
            Layout.preferredWidth: Kirigami.Units.iconSizes.small
            Layout.preferredHeight: Kirigami.Units.iconSizes.small
            Kirigami.Icon {
                anchors.fill: parent
                visible: row.app.sandbox !== "none"
                source: "security-high"
                isMask: true
                color: row.app.sandbox === "strict" ? Kirigami.Theme.positiveTextColor : Kirigami.Theme.highlightColor
            }
        }
        Text {
            text: row.app.version || ""
            color: Kirigami.Theme.textColor
            opacity: 0.6
            font: Kirigami.Theme.smallFont
            elide: Text.ElideRight
            Layout.fillWidth: true
        }
        // What matters, in a few words.
        Text {
            visible: !row.narrow && !row.busy && (row.hasUpdate || row.hasProblem)
            text: row.hasUpdate ? qsTr("Update: %1").arg(row.app.latest) : qsTr("Check failed")
            color: row.hasUpdate ? Kirigami.Theme.positiveTextColor : Kirigami.Theme.neutralTextColor
            font.weight: Font.DemiBold
            font.pointSize: Kirigami.Theme.defaultFont.pointSize * 0.92
            elide: Text.ElideRight
            HoverHandler { id: statusHover }
            QQC2.ToolTip.visible: statusHover.hovered && row.hasProblem
            QQC2.ToolTip.text: row.app.error || ""
        }
        QQC2.ProgressBar {
            visible: row.busy
            indeterminate: row.downloading < 0
            value: Math.max(0, row.downloading)
            Layout.preferredWidth: Kirigami.Units.gridUnit * 6
        }
        // A dot stands in for the words when the line is narrow.
        Rectangle {
            visible: row.narrow && !row.busy && (row.hasUpdate || row.hasProblem)
            Layout.preferredWidth: 8; Layout.preferredHeight: 8; radius: 4
            color: row.hasUpdate ? Kirigami.Theme.positiveTextColor : Kirigami.Theme.neutralTextColor
        }

        PrimaryButton {
            visible: row.hasUpdate && !row.busy
            text: qsTr("Update")
            icon.name: "software-update-available"
            display: QQC2.AbstractButton.IconOnly
            Layout.preferredWidth: implicitHeight
            QQC2.ToolTip.visible: hovered
            QQC2.ToolTip.text: qsTr("Update to %1").arg(row.app.latest)
            Accessible.name: qsTr("Update %1").arg(row.app.name)
            onClicked: applicationWindow().backendObject.start("update", row.app.id, "", "", "")
        }
        QQC2.ToolButton {
            text: qsTr("Open")
            icon.name: "media-playback-start"
            display: QQC2.AbstractButton.IconOnly
            QQC2.ToolTip.visible: hovered
            QQC2.ToolTip.text: qsTr("Open %1").arg(row.app.name)
            Accessible.name: qsTr("Open %1").arg(row.app.name)
            onClicked: {
                var err = applicationWindow().backendObject.launch(row.app.id)
                if (err) applicationWindow().showPassiveNotification(err)
            }
        }
        AppMenuButton { app: row.app; busy: row.busy }
    }
}
