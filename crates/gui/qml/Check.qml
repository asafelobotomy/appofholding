import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami

// A check box that stays readable in our own colour themes (see Radio.qml for why). With
// "follow the system" it is the desktop's own.
QQC2.CheckBox {
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
            radius: Kirigami.Units.cornerRadius / 2
            color: control.checked ? p.accent : p.bg
            border.width: control.visualFocus ? 2 : 1
            border.color: control.checked || control.visualFocus || control.hovered ? p.accent : p.dim
            opacity: control.enabled ? 1 : 0.5
            // Drawn as text: icon themes name the check-mark icon differently, and a missing icon
            // would draw as a solid block over the fill.
            Text {
                anchors.centerIn: parent
                text: "\u2713"
                font.pixelSize: Math.round(parent.height * 0.85)
                font.bold: true
                color: control.themeColors.accentText
                visible: control.checked
            }
        }
    }
}
