import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// A titled block of a detail page: heading, optional subtitle, then content.
ColumnLayout {
    id: section
    property string title
    property string subtitle
    property bool first: false
    default property alias content: body.data

    Layout.fillWidth: true
    spacing: Kirigami.Units.smallSpacing

    Kirigami.Separator { visible: !section.first; Layout.fillWidth: true; Layout.topMargin: Kirigami.Units.largeSpacing }
    Kirigami.Heading { text: section.title; level: 2; Layout.topMargin: Kirigami.Units.smallSpacing }
    QQC2.Label {
        visible: section.subtitle.length > 0
        text: section.subtitle
        wrapMode: Text.Wrap
        opacity: 0.7
        Layout.fillWidth: true
    }
    ColumnLayout {
        id: body
        Layout.fillWidth: true
        spacing: Kirigami.Units.smallSpacing
    }
}
