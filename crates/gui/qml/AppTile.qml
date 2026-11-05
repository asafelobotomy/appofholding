import QtQuick
import org.kde.kirigami as Kirigami

// An app icon on a soft rounded tile, so icons of different shapes look like one family.
Rectangle {
    id: tile
    property var source
    property int size: Kirigami.Units.iconSizes.huge + Kirigami.Units.smallSpacing * 2

    implicitWidth: size
    implicitHeight: size
    radius: size * 0.24
    color: Kirigami.ColorUtils.tintWithAlpha(Kirigami.Theme.backgroundColor, Kirigami.Theme.textColor, 0.07)
    border.width: 1
    border.color: Kirigami.ColorUtils.tintWithAlpha(Kirigami.Theme.backgroundColor, Kirigami.Theme.textColor, 0.12)

    Kirigami.Icon {
        anchors.centerIn: parent
        width: tile.size * 0.74
        height: width
        source: tile.source || "application-x-executable"
        fallback: "application-x-executable"
    }
}
