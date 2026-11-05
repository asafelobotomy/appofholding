import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami

// The one button on a screen that you most likely want. It is tinted so it stands out from the
// plain buttons around it in light and dark themes alike. Tinting goes through the palette, which
// every style honours, rather than a custom background.
QQC2.Button {
    id: button
    property color tone: Kirigami.Theme.positiveTextColor
    font.weight: Font.DemiBold
    palette.button: Kirigami.ColorUtils.tintWithAlpha(Kirigami.Theme.backgroundColor, tone, button.down ? 0.55 : button.hovered ? 0.42 : 0.32)
}
