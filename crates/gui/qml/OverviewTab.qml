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

    // Simple improvements for this app on this computer, each one click to apply or dismiss.
    property var suggestions: []
    function loadSuggestions() {
        if (!tab.appId || !tab.backend) return
        var r = Util.parse(tab.backend.suggestions(tab.appId))
        tab.suggestions = r.error ? [] : r
    }
    onDetailChanged: loadSuggestions()
    onAppIdChanged: loadSuggestions()

    SectionCard {
        visible: tab.suggestions.length > 0
        title: qsTr("Suggestions")
        iconName: "tools-wizard"
        subtitle: qsTr("Small changes that may make this app work better here. Nothing changes unless you choose it.")

        Repeater {
            model: tab.suggestions
            ColumnLayout {
                required property var modelData
                required property int index
                Layout.fillWidth: true
                Layout.topMargin: index > 0 ? Kirigami.Units.smallSpacing : 0
                spacing: Kirigami.Units.smallSpacing

                Kirigami.Separator { visible: index > 0; Layout.fillWidth: true }
                QQC2.Label {
                    text: modelData.title
                    font.weight: Font.DemiBold
                    wrapMode: Text.Wrap
                    Layout.fillWidth: true
                }
                QQC2.Label {
                    text: modelData.detail
                    wrapMode: Text.Wrap
                    opacity: 0.8
                    Layout.fillWidth: true
                }
                RowLayout {
                    spacing: Kirigami.Units.smallSpacing
                    PrimaryButton {
                        text: modelData.action
                        icon.name: "dialog-ok-apply"
                        onClicked: {
                            var r = Util.parse(tab.backend.applySuggestion(tab.appId, modelData.key))
                            applicationWindow().showPassiveNotification(r.error ? r.error : r.message, r.error ? "long" : "short")
                            applicationWindow().reload()
                        }
                    }
                    QQC2.Button {
                        visible: modelData.dismissible
                        text: qsTr("Not for this app")
                        icon.name: "dialog-cancel"
                        flat: true
                        onClicked: {
                            var err = tab.backend.dismissSuggestion(tab.appId, modelData.key)
                            if (err) applicationWindow().showPassiveNotification(err)
                            tab.loadSuggestions()
                        }
                    }
                }
            }
        }
    }

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
                text: !tab.src ? qsTr("Add a link to get new versions automatically.")
                    : (tab.src.lastChecked ? qsTr("Checked %1").arg(Util.ago(tab.src.lastChecked)) : qsTr("Never checked"))
                      + " · " + qsTr("from %1").arg(tab.src.describe)
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
                + (l.cwd ? 1 : 0) + (l.extractAndRun ? 1 : 0) + (l.elevated ? 1 : 0)
            Text {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                font.weight: Font.DemiBold
                font.pointSize: Kirigami.Theme.defaultFont.pointSize * 1.05
                color: launchCard.l.elevated ? Kirigami.Theme.neutralTextColor : Kirigami.Theme.textColor
                text: launchCard.l.elevated ? qsTr("Runs as administrator")
                    : launchCard.count === 0 ? qsTr("Default launch") : qsTr("%n change(s) from the default", "", launchCard.count)
            }
            Text {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                opacity: 0.7
                color: Kirigami.Theme.textColor
                font: Kirigami.Theme.smallFont
                text: (tab.detail.profiles && tab.detail.profiles.length > 1)
                    ? qsTr("%n extra profile(s) in the right-click menu", "", tab.detail.profiles.length - 1)
                    : (launchCard.l.presets || []).length > 0 ? qsTr("Quick fixes: %1").arg(launchCard.l.presets.join(", "))
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
                readonly property var sb: tab.detail.sandbox || ({})
                text: sandboxCard.level === "none" ? qsTr("It can read your files and use the network.")
                    : [sb.network ? qsTr("network") : qsTr("no network"),
                       sb.home === "private" ? qsTr("private home") : sb.home === "ro" ? qsTr("reads your home") : qsTr("writes your home"),
                       qsTr("%n shared folder(s)", "", (sb.paths || []).length)].join(" · ")
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
                onClicked: {
                    var dir = (tab.detail.path || "").replace(/\/[^\/]*$/, "")
                    Qt.openUrlExternally("file://" + dir.split("/").map(encodeURIComponent).join("/"))
                }
            }

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

            QQC2.Label { text: qsTr("App ID"); opacity: 0.7 }
            QQC2.Label {
                text: tab.detail.id || ""
                font.family: "monospace"
                Layout.fillWidth: true
                Layout.columnSpan: 2
                QQC2.ToolTip.visible: idHover.hovered
                QQC2.ToolTip.text: qsTr("Use this name with the appofholding command, e.g. appofholding update %1").arg(tab.detail.id || "")
                HoverHandler { id: idHover }
            }

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
