import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// A titled card for one topic of a page. Children go inside, below the heading.
Rectangle {
    id: card
    property string title
    property string subtitle
    property string iconName
    default property alias content: body.data

    Layout.fillWidth: true
    implicitHeight: column.implicitHeight + Kirigami.Units.largeSpacing * 2
    radius: Kirigami.Units.cornerRadius
    color: Kirigami.ColorUtils.tintWithAlpha(Kirigami.Theme.backgroundColor, Kirigami.Theme.textColor, 0.045)
    border.width: 1
    border.color: Kirigami.ColorUtils.tintWithAlpha(Kirigami.Theme.backgroundColor, Kirigami.Theme.textColor, 0.12)

    ColumnLayout {
        id: column
        anchors {
            left: parent.left; right: parent.right; top: parent.top
            margins: Kirigami.Units.largeSpacing
        }
        spacing: Kirigami.Units.smallSpacing

        RowLayout {
            visible: card.title.length > 0
            spacing: Kirigami.Units.smallSpacing
            Kirigami.Icon {
                visible: card.iconName.length > 0
                source: card.iconName
                Layout.preferredWidth: Kirigami.Units.iconSizes.smallMedium
                Layout.preferredHeight: Kirigami.Units.iconSizes.smallMedium
            }
            Kirigami.Heading {
                text: card.title
                level: 3
                font.weight: Font.DemiBold
                Layout.fillWidth: true
            }
        }
        Text {
            visible: card.subtitle.length > 0
            text: card.subtitle
            wrapMode: Text.Wrap
            color: Kirigami.Theme.textColor
            opacity: 0.7
            font: Kirigami.Theme.defaultFont
            Layout.fillWidth: true
        }
        ColumnLayout {
            id: body
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing
        }
    }
}
