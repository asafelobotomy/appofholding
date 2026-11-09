import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

ThemedPage {
    id: page

    property string appId
    property var d: ({})
    property string error
    readonly property var backend: applicationWindow().backendObject
    readonly property var src: d.source || null
    readonly property real downloading: applicationWindow().progress[appId] !== undefined ? applicationWindow().progress[appId] : -2
    readonly property bool hasUpdate: !!src && src.status === "update-available"
    // Lets other parts jump to a tab (e.g. "Change source" from the overview).
    property alias tabIndex: tabs.currentIndex

    title: d.name || appId

    function load() {
        var r = Util.parse(backend.detail(appId))
        if (r.error) {
            error = r.error
            d = ({})
        } else {
            error = ""
            d = r
        }
    }

    Component.onCompleted: load()

    Connections {
        target: applicationWindow()
        function onRevisionChanged() { page.load() }
    }

    // "Open" and "Update" are the buttons beside the app's name.
    actions: [
        Kirigami.Action {
            text: qsTr("Remove…")
            icon.name: "edit-delete"
            onTriggered: removeDialog.open()
        }
    ]

    function openApp() {
        var err = backend.launch(appId)
        if (err) applicationWindow().showPassiveNotification(err)
    }

    ColumnLayout {
        spacing: Kirigami.Units.largeSpacing

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: page.error.length > 0
            type: Kirigami.MessageType.Error
            text: page.error
        }

        // ---- hero
        RowLayout {
            Layout.fillWidth: true
            visible: !!page.d.id
            spacing: Kirigami.Units.gridUnit

            AppTile {
                source: page.d.icon
                size: Kirigami.Units.iconSizes.huge * 1.7
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing
                Kirigami.Heading {
                    text: page.d.name || ""
                    level: 1
                    font.weight: Font.DemiBold
                    elide: Text.ElideRight
                    Layout.fillWidth: true
                }
                Text {
                    text: (page.d.version ? qsTr("Version %1").arg(page.d.version) : qsTr("Version unknown"))
                        + (page.d.size ? "  ·  " + Util.fmtSize(page.d.size) : "")
                    color: Kirigami.Theme.textColor
                    opacity: 0.7
                    font: Kirigami.Theme.defaultFont
                }
                Flow {
                    spacing: Kirigami.Units.smallSpacing
                    Layout.fillWidth: true
                    Pill {
                        visible: page.hasUpdate
                        tone: "positive"
                        iconName: "software-update-available"
                        text: qsTr("Update available: %1").arg(page.src ? page.src.latest : "")
                    }
                    Pill {
                        visible: !!page.src && page.src.status === "up-to-date"
                        tone: "positive"
                        iconName: "emblem-ok-symbolic"
                        text: qsTr("Up to date")
                    }
                    Pill {
                        visible: !!page.src && !!page.src.error
                        tone: "warning"
                        iconName: "dialog-warning"
                        text: qsTr("Couldn't check for updates")
                    }
                    Pill {
                        visible: !!page.d.sandbox && page.d.sandbox.level !== "none"
                        tone: page.d.sandbox && page.d.sandbox.level === "strict" ? "positive" : "info"
                        iconName: "security-high"
                        text: page.d.sandbox ? Util.sandboxLabel(page.d.sandbox.level) : ""
                    }
                    Pill {
                        visible: !!page.d.running
                        tone: "info"
                        iconName: "media-playback-start"
                        text: qsTr("Running")
                    }
                }
            }
            ColumnLayout {
                spacing: Kirigami.Units.smallSpacing
                Layout.alignment: Qt.AlignTop
                PrimaryButton {
                    visible: page.hasUpdate
                    enabled: page.downloading <= -2 && !applicationWindow().working
                    text: qsTr("Update")
                    icon.name: "software-update-available"
                    Layout.fillWidth: true
                    onClicked: page.backend.start("update", page.appId, "", "", "")
                }
                QQC2.Button {
                    text: qsTr("Open")
                    icon.name: "media-playback-start"
                    flat: page.hasUpdate
                    Layout.fillWidth: true
                    onClicked: page.openApp()
                }
            }
        }

        QQC2.ProgressBar {
            visible: page.downloading > -2
            indeterminate: page.downloading < 0
            value: Math.max(0, page.downloading)
            Layout.fillWidth: true
        }

        // ---- tabs
        QQC2.TabBar {
            id: tabs
            visible: !!page.d.id
            Layout.fillWidth: true
            QQC2.TabButton { text: qsTr("Overview"); icon.name: "view-list-details"; width: tabs.width / 4 }
            QQC2.TabButton { text: qsTr("Updates"); icon.name: "software-update-available"; width: tabs.width / 4 }
            QQC2.TabButton { text: qsTr("Launch"); icon.name: "media-playback-start"; width: tabs.width / 4 }
            QQC2.TabButton { text: qsTr("Sandbox"); icon.name: "security-high"; width: tabs.width / 4 }
        }

        OverviewTab {
            visible: tabs.currentIndex === 0 && !!page.d.id
            detail: page.d
            appId: page.appId
            backend: page.backend
            Layout.fillWidth: true
            onGoToTab: (i) => tabs.currentIndex = i
        }
        SourceSection { visible: tabs.currentIndex === 1 && !!page.d.id; detail: page.d; appId: page.appId; backend: page.backend; Layout.fillWidth: true }
        LaunchSection { id: launchTab; visible: tabs.currentIndex === 2 && !!page.d.id; detail: page.d; appId: page.appId; backend: page.backend; Layout.fillWidth: true }
        SandboxSection { id: sandboxTab; visible: tabs.currentIndex === 3 && !!page.d.id; detail: page.d; appId: page.appId; backend: page.backend; Layout.fillWidth: true }
    }

    // Unsaved launch or sandbox changes stay in sight below the page, whichever tab is open
    // and however far it is scrolled.
    footer: QQC2.Control {
        visible: launchTab.dirty || sandboxTab.dirty
        padding: Kirigami.Units.smallSpacing
        contentItem: ColumnLayout {
            spacing: Kirigami.Units.smallSpacing
            Kirigami.InlineMessage {
                Layout.fillWidth: true
                visible: launchTab.dirty
                type: Kirigami.MessageType.Information
                text: qsTr("Unsaved launch settings")
                actions: [
                    Kirigami.Action { text: qsTr("Save"); icon.name: "document-save"; onTriggered: launchTab.save() },
                    Kirigami.Action { text: qsTr("Revert"); icon.name: "edit-undo"; onTriggered: launchTab.revert() },
                    Kirigami.Action { text: qsTr("Show"); icon.name: "go-next"; visible: tabs.currentIndex !== 2; onTriggered: tabs.currentIndex = 2 }
                ]
            }
            Kirigami.InlineMessage {
                Layout.fillWidth: true
                visible: sandboxTab.dirty
                type: Kirigami.MessageType.Information
                text: qsTr("Unsaved sandbox settings")
                actions: [
                    Kirigami.Action { text: qsTr("Save"); icon.name: "document-save"; onTriggered: sandboxTab.save() },
                    Kirigami.Action { text: qsTr("Revert"); icon.name: "edit-undo"; onTriggered: sandboxTab.revert() },
                    Kirigami.Action { text: qsTr("Show"); icon.name: "go-next"; visible: tabs.currentIndex !== 3; onTriggered: tabs.currentIndex = 3 }
                ]
            }
        }
    }

    QQC2.Dialog {
        id: removeDialog
        title: qsTr("Remove %1?").arg(page.d.name || page.appId)
        modal: true
        anchors.centerIn: QQC2.Overlay.overlay
        standardButtons: QQC2.Dialog.Yes | QQC2.Dialog.Cancel
        contentItem: ColumnLayout {
            spacing: Kirigami.Units.largeSpacing
            QQC2.Label {
                text: page.d.externalFile
                    ? qsTr("This removes the menu entry. The AppImage file is yours and stays where it is: %1").arg(page.d.path)
                    : qsTr("This removes the menu entry and the AppImage file.")
                wrapMode: Text.Wrap
                Layout.fillWidth: true
                Layout.maximumWidth: Kirigami.Units.gridUnit * 28
            }
            QQC2.CheckBox {
                id: purge
                text: qsTr("Also delete the app's private data (cannot be undone)")
                visible: page.d.sandbox && page.d.sandbox.level !== "none"
            }
        }
        onAccepted: {
            var err = page.backend.remove(page.appId, purge.checked)
            if (err) {
                applicationWindow().showPassiveNotification(err)
            } else {
                applicationWindow().reload()
                applicationWindow().pageStack.pop()
            }
        }
    }
}
