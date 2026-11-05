import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// The first tab: everything about the app at a glance, with shortcuts to the other tabs.
ColumnLayout {
    id: tab
    property var detail: ({})
    property string appId
    property var backend
    readonly property var src: detail.source || null
    signal goToTab(int index)

    spacing: Kirigami.Units.largeSpacing

    // ---- at a glance: three small summaries that each lead to a tab
    GridLayout {
        Layout.fillWidth: true
        columns: width > Kirigami.Units.gridUnit * 34 ? 3 : 1
        columnSpacing: Kirigami.Units.largeSpacing
        rowSpacing: Kirigami.Units.largeSpacing

        SectionCard {
            id: updatesCard
            title: qsTr("Updates")
            iconName: "update-none"
            Layout.alignment: Qt.AlignTop
            Text {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: !tab.src ? Kirigami.Theme.textColor
                    : tab.src.status === "update-available" ? Kirigami.Theme.positiveTextColor
                    : tab.src.error ? Kirigami.Theme.neutralTextColor : Kirigami.Theme.textColor
                font.weight: Font.DemiBold
                font.pointSize: Kirigami.Theme.defaultFont.pointSize * 1.05
                text: !tab.src ? qsTr("No update source")
                    : tab.src.status === "update-available" ? qsTr("%1 is available").arg(tab.src.latest)
                    : tab.src.error ? qsTr("The last check failed")
                    : tab.src.status === "up-to-date" ? qsTr("Up to date") : qsTr("Not checked yet")
            }
            Text {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                opacity: 0.7
                color: Kirigami.Theme.textColor
                font: Kirigami.Theme.smallFont
                text: tab.src ? qsTr("Checked %1 · from %2").arg(Util.ago(tab.src.lastChecked)).arg(tab.src.describe) : qsTr("Add a link to get new versions automatically.")
            }
            QQC2.Button {
                flat: true
                text: qsTr("Manage updates")
                icon.name: "go-next"
                LayoutMirroring.enabled: true
                onClicked: tab.goToTab(1)
            }
        }

        SectionCard {
            id: launchCard
            title: qsTr("Launch")
            iconName: "media-playback-start"
            Layout.alignment: Qt.AlignTop
            readonly property var l: tab.detail.launch || ({})
            readonly property int count: ((l.presets || []).length) + (l.args ? 1 : 0) + (l.env ? 1 : 0) + (l.prefix ? 1 : 0)
            Text {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                font.weight: Font.DemiBold
                font.pointSize: Kirigami.Theme.defaultFont.pointSize * 1.05
                color: Kirigami.Theme.textColor
                text: launchCard.count === 0 ? qsTr("Default launch") : qsTr("Customised")
            }
            Text {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                opacity: 0.7
                color: Kirigami.Theme.textColor
                font: Kirigami.Theme.smallFont
                text: (tab.detail.profiles && tab.detail.profiles.length > 1)
                    ? qsTr("%1 extra profile(s) in the right-click menu").arg(tab.detail.profiles.length - 1)
                    : qsTr("Arguments, environment and ready-made fixes.")
            }
            QQC2.Button {
                flat: true
                text: qsTr("Launch options")
                icon.name: "go-next"
                LayoutMirroring.enabled: true
                onClicked: tab.goToTab(2)
            }
        }

        SectionCard {
            id: sandboxCard
            title: qsTr("Sandbox")
            iconName: "security-high"
            Layout.alignment: Qt.AlignTop
            readonly property string level: tab.detail.sandbox ? tab.detail.sandbox.level : "none"
            Text {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                font.weight: Font.DemiBold
                font.pointSize: Kirigami.Theme.defaultFont.pointSize * 1.05
                color: sandboxCard.level === "none" ? Kirigami.Theme.neutralTextColor : Kirigami.Theme.positiveTextColor
                text: sandboxCard.level === "none" ? qsTr("Not sandboxed") : Util.sandboxLabel(sandboxCard.level)
            }
            Text {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                opacity: 0.7
                color: Kirigami.Theme.textColor
                font: Kirigami.Theme.smallFont
                text: sandboxCard.level === "none" ? qsTr("It can read your files and use the network.") : qsTr("Limited access to your files, network and devices.")
            }
            QQC2.Button {
                flat: true
                text: qsTr("Protection settings")
                icon.name: "go-next"
                LayoutMirroring.enabled: true
                onClicked: tab.goToTab(3)
            }
        }
    }

    // ---- the file
    SectionCard {
        title: qsTr("File")
        iconName: "document-properties"

        GridLayout {
            columns: 3
            columnSpacing: Kirigami.Units.largeSpacing
            rowSpacing: Kirigami.Units.smallSpacing
            Layout.fillWidth: true

            QQC2.Label { text: qsTr("Location"); opacity: 0.7 }
            QQC2.Label { text: tab.detail.path || ""; elide: Text.ElideMiddle; Layout.fillWidth: true; font.family: "monospace" }
            QQC2.ToolButton {
                icon.name: "folder-open"
                Accessible.name: qsTr("Show in folder")
                QQC2.ToolTip.visible: hovered
                QQC2.ToolTip.text: qsTr("Show in folder")
                onClicked: Qt.openUrlExternally("file://" + (tab.detail.path || "").replace(/\/[^\/]*$/, ""))
            }

            QQC2.Label { text: qsTr("Size"); opacity: 0.7 }
            QQC2.Label { text: Util.fmtSize(tab.detail.size); Layout.columnSpan: 2 }

            QQC2.Label { text: qsTr("SHA-256"); opacity: 0.7 }
            QQC2.Label { text: tab.detail.sha256 || ""; elide: Text.ElideMiddle; Layout.fillWidth: true; font.family: "monospace" }
            QQC2.ToolButton {
                icon.name: "edit-copy"
                Accessible.name: qsTr("Copy checksum")
                QQC2.ToolTip.visible: hovered
                QQC2.ToolTip.text: qsTr("Copy")
                onClicked: { hidden.text = tab.detail.sha256 || ""; hidden.selectAll(); hidden.copy(); applicationWindow().showPassiveNotification(qsTr("Checksum copied")) }
            }

            QQC2.Label { text: qsTr("Menu entry"); opacity: 0.7 }
            QQC2.Label { text: tab.detail.desktopFile || ""; elide: Text.ElideMiddle; Layout.fillWidth: true; font.family: "monospace"; Layout.columnSpan: 2 }

            QQC2.Label { text: qsTr("Ownership"); opacity: 0.7; visible: !!tab.detail.externalFile }
            QQC2.Label {
                visible: !!tab.detail.externalFile
                text: qsTr("Your file, kept where it was. Removing the app leaves it in place.")
                wrapMode: Text.Wrap
                Layout.fillWidth: true
                Layout.columnSpan: 2
            }
        }
        QQC2.TextField { id: hidden; visible: false }
    }
}
