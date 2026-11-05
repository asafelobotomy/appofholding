import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// A small rounded status label. tone: positive | warning | negative | info | neutral
Rectangle {
    id: pill
    property string text
    property string iconName
    property string tone: "neutral"
    readonly property color accent: tone === "positive" ? Kirigami.Theme.positiveTextColor
        : tone === "warning" ? Kirigami.Theme.neutralTextColor
        : tone === "negative" ? Kirigami.Theme.negativeTextColor
        : tone === "info" ? Kirigami.Theme.highlightColor
        : Kirigami.Theme.disabledTextColor

    implicitHeight: row.implicitHeight + Kirigami.Units.smallSpacing
    implicitWidth: row.implicitWidth + Kirigami.Units.largeSpacing
    radius: height / 2
    color: Kirigami.ColorUtils.tintWithAlpha(Kirigami.Theme.backgroundColor, accent, 0.16)
    border.width: 1
    border.color: Kirigami.ColorUtils.tintWithAlpha(Kirigami.Theme.backgroundColor, accent, 0.4)

    RowLayout {
        id: row
        anchors.centerIn: parent
        spacing: Kirigami.Units.smallSpacing / 2
        Kirigami.Icon {
            visible: pill.iconName.length > 0
            source: pill.iconName
            color: pill.accent
            isMask: true
            Layout.preferredWidth: Kirigami.Units.iconSizes.small
            Layout.preferredHeight: Kirigami.Units.iconSizes.small
        }
        Text {
            text: pill.text
            color: pill.accent
            font.pointSize: Kirigami.Theme.defaultFont.pointSize * 0.9
            font.weight: Font.DemiBold
        }
    }
}
