import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami

// A radio button that stays readable in our own colour themes. The style used with them
// (Fusion) draws the mark from the base and text colours, which comes out faint on Gruvbox; this
// draws it in the theme's accent instead. With "follow the system" it is the desktop's own.
QQC2.RadioButton {
    id: control
    readonly property var themeColors: applicationWindow() ? applicationWindow().themePalette : null
    Component.onCompleted: if (themeColors) indicator = mark.createObject(control)

    Component {
        id: mark
        Rectangle {
            readonly property var p: control.themeColors
            implicitWidth: Kirigami.Units.iconSizes.small
            implicitHeight: implicitWidth
            x: control.mirrored ? control.width - width - control.rightPadding : control.leftPadding
            y: control.topPadding + (control.availableHeight - height) / 2
            radius: width / 2
            color: p.bg
            border.width: control.checked || control.visualFocus ? 2 : 1
            border.color: control.checked || control.visualFocus || control.hovered ? p.accent : p.dim
            opacity: control.enabled ? 1 : 0.5
            Rectangle {
                anchors.centerIn: parent
                width: parent.width / 2
                height: width
                radius: width / 2
                color: control.themeColors.accent
                visible: control.checked
            }
        }
    }
}
