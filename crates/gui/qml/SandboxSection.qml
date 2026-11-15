import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import QtQuick.Dialogs
import org.kde.kirigami as Kirigami

// The "Sandbox" tab.
ColumnLayout {
    id: sec
    property var detail: ({})
    property string appId
    property var backend
    property var s: ({level: "none", paths: []})
    property bool dirty: false

    readonly property bool enabledSandbox: s.level !== "none"

    spacing: Kirigami.Units.largeSpacing

    function notify(err) { if (err) applicationWindow().showPassiveNotification(err) }

    onDetailChanged: if (detail.sandbox && !dirty) s = JSON.parse(JSON.stringify(detail.sandbox))

    function setLevel(level) {
        var r = Util.parse(backend.sandboxDefaults(level))
        if (r.error) { notify(r.error); return }
        r.paths = s.paths || []
        r.level = level
        s = r
        dirty = true
    }

    function change(key, value) {
        var c = JSON.parse(JSON.stringify(s))
        c[key] = value
        if (c.level === "standard" || c.level === "strict") c.level = "custom"
        s = c
        dirty = true
    }

    function save() {
        var r = Util.parse(backend.saveSandbox(appId, JSON.stringify(s)))
        if (r.error) { notify(r.error); return }
        s = r
        dirty = false
        applicationWindow().showPassiveNotification(qsTr("Sandbox settings saved"))
        applicationWindow().reload()
    }

    function revert() {
        dirty = false
        if (detail.sandbox) s = JSON.parse(JSON.stringify(detail.sandbox))
    }

    // ---- protection level
    SectionCard {
        title: qsTr("Protection")
        iconName: "security-high"
        subtitle: sec.enabledSandbox
            ? (sec.s.home === "ro" || sec.s.home === "rw"
                ? qsTr("Limits what this app can see and do. It runs from an unpacked copy and can see your home folder (see Files below).")
                : qsTr("Limits what this app can see and do. It runs from an unpacked copy with its own private home folder."))
            : qsTr("Off: the app runs like any other program, with access to your files, network and devices. Pick Standard or Strict to limit it.")

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing
            Repeater {
                model: [
                    {key: "none", label: qsTr("Off"), icon: "security-low", text: qsTr("Like any other program")},
                    {key: "standard", label: qsTr("Standard"), icon: "security-medium", text: qsTr("Works normally; can't see your files")},
                    {key: "strict", label: qsTr("Strict"), icon: "security-high", text: qsTr("No network or sound as well")}
                ]
                Rectangle {
                    id: tile
                    required property var modelData
                    readonly property bool selected: sec.s.level === modelData.key
                    readonly property bool available: true
                    Layout.fillWidth: true
                    Layout.preferredWidth: 1
                    implicitHeight: col.implicitHeight + Kirigami.Units.largeSpacing * 2
                    radius: Kirigami.Units.cornerRadius
                    opacity: available ? 1 : 0.4
                    color: selected ? Kirigami.ColorUtils.tintWithAlpha(Kirigami.Theme.backgroundColor, Kirigami.Theme.highlightColor, 0.2)
                                    : Kirigami.ColorUtils.tintWithAlpha(Kirigami.Theme.backgroundColor, Kirigami.Theme.textColor, tileHover.hovered && available ? 0.1 : 0.04)
                    border.width: selected ? 2 : 1
                    border.color: selected ? Kirigami.Theme.highlightColor : Kirigami.ColorUtils.tintWithAlpha(Kirigami.Theme.backgroundColor, Kirigami.Theme.textColor, 0.15)
                    activeFocusOnTab: available
                    Accessible.role: Accessible.RadioButton
                    Accessible.name: modelData.label
                    Accessible.checked: selected
                    Keys.onSpacePressed: sec.setLevel(modelData.key)

                    HoverHandler { id: tileHover; cursorShape: tile.available ? Qt.PointingHandCursor : Qt.ArrowCursor }
                    TapHandler { onTapped: sec.setLevel(tile.modelData.key) }

                    ColumnLayout {
                        id: col
                        anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter; margins: Kirigami.Units.largeSpacing }
                        spacing: Kirigami.Units.smallSpacing
                        Kirigami.Icon {
                            source: tile.modelData.icon
                            Layout.alignment: Qt.AlignHCenter
                            Layout.preferredWidth: Kirigami.Units.iconSizes.medium
                            Layout.preferredHeight: Kirigami.Units.iconSizes.medium
                        }
                        Kirigami.Heading { text: tile.modelData.label; level: 4; font.weight: Font.DemiBold; Layout.alignment: Qt.AlignHCenter }
                        Text {
                            text: tile.modelData.text
                            wrapMode: Text.Wrap
                            horizontalAlignment: Text.AlignHCenter
                            color: Kirigami.Theme.textColor
                            opacity: 0.7
                            font: Kirigami.Theme.smallFont
                            Layout.fillWidth: true
                        }
                    }
                }
            }
        }
        // Not a level to pick: what a preset becomes once you change one of its switches.
        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: sec.s.level === "custom"
            type: Kirigami.MessageType.Information
            text: qsTr("Custom: your own mix of the switches below. Pick a level above to start again from it.")
        }
    }

    // ---- permissions (only meaningful with a sandbox)
    SectionCard {
        visible: sec.enabledSandbox
        title: qsTr("Devices")
        iconName: "preferences-system-privacy"
        subtitle: qsTr("What the app may use.")

        Repeater {
            model: [
                {key: "network", icon: "network-wireless", label: qsTr("Network"), text: qsTr("Connect to the internet")},
                {key: "display", icon: "video-display", label: qsTr("Windows"), text: qsTr("Open windows on your desktop")},
                {key: "gpu", icon: "cpu", label: qsTr("Graphics card"), text: qsTr("Hardware acceleration")},
                {key: "audio", icon: "audio-volume-high", label: qsTr("Sound"), text: qsTr("Play and record audio")},
                {key: "devices", icon: "camera-web", label: qsTr("Camera and media devices"), text: qsTr("Webcams and similar")}
            ]
            RowLayout {
                required property var modelData
                Layout.fillWidth: true
                spacing: Kirigami.Units.largeSpacing
                Kirigami.Icon {
                    source: modelData.icon
                    Layout.preferredWidth: Kirigami.Units.iconSizes.smallMedium
                    Layout.preferredHeight: Kirigami.Units.iconSizes.smallMedium
                }
                ColumnLayout {
                    spacing: 0
                    Layout.fillWidth: true
                    // Filling labels let the column grow, which keeps every switch at the edge.
                    QQC2.Label { text: modelData.label; font.weight: Font.DemiBold; Layout.fillWidth: true; elide: Text.ElideRight }
                    QQC2.Label { text: modelData.text; opacity: 0.7; font: Kirigami.Theme.smallFont; Layout.fillWidth: true; elide: Text.ElideRight }
                }
                QQC2.Switch {
                    checked: !!sec.s[modelData.key]
                    Accessible.name: modelData.label
                    onToggled: sec.change(modelData.key, checked)
                }
            }
        }

    }

    // ---- talking to the rest of the desktop
    SectionCard {
        visible: sec.enabledSandbox
        title: qsTr("Desktop integration")
        iconName: "preferences-desktop"
        subtitle: qsTr("Which desktop services the app can reach over the session bus.")
        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.largeSpacing
            QQC2.Label { text: qsTr("Desktop services"); font.weight: Font.DemiBold; Layout.fillWidth: true }
            QQC2.ComboBox {
                readonly property var keys: ["off", "filtered", "full"]
                Layout.maximumWidth: Kirigami.Units.gridUnit * 18
                model: [qsTr("None"), qsTr("File dialogs, notifications, tray icon"), qsTr("Everything (unsafe)")]
                currentIndex: Math.max(0, keys.indexOf(sec.s.bus))
                onActivated: (i) => sec.change("bus", keys[i])
                Accessible.name: qsTr("Desktop services")
            }
        }
        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: sec.s.bus === "full"
            type: Kirigami.MessageType.Warning
            text: qsTr("With full desktop services the app can talk to every program on your session.")
        }
    }

    // ---- files: home folder and shared folders
    SectionCard {
        visible: sec.enabledSandbox
        title: qsTr("Files")
        iconName: "folder-home"

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.largeSpacing
            QQC2.Label { text: qsTr("Home folder"); font.weight: Font.DemiBold; Layout.fillWidth: true }
            QQC2.ComboBox {
                readonly property var keys: ["private", "ro", "rw"]
                Layout.maximumWidth: Kirigami.Units.gridUnit * 18
                model: [qsTr("Its own empty folder"), qsTr("Mine, read-only"), qsTr("Mine, read and write")]
                currentIndex: Math.max(0, keys.indexOf(sec.s.home))
                onActivated: (i) => sec.change("home", keys[i])
                Accessible.name: qsTr("Home folder")
            }
        }
        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: sec.s.home === "rw"
            type: Kirigami.MessageType.Warning
            text: qsTr("With a writable home folder the app can change any of your files.")
        }

        Kirigami.Separator { Layout.fillWidth: true; Layout.topMargin: Kirigami.Units.smallSpacing }
        QQC2.Label { text: qsTr("Shared folders"); font.weight: Font.DemiBold }
        QQC2.Label {
            text: (sec.s.paths || []).length === 0
                ? qsTr("None. Share a folder to let the app open files outside its home.")
                : qsTr("The app can open these as well as its home folder.")
            opacity: 0.7
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
        Repeater {
            model: sec.s.paths || []
            RowLayout {
                required property var modelData
                required property int index
                Layout.fillWidth: true
                Kirigami.Icon { source: "folder"; Layout.preferredWidth: Kirigami.Units.iconSizes.smallMedium; Layout.preferredHeight: Kirigami.Units.iconSizes.smallMedium }
                QQC2.Label { text: modelData.path; elide: Text.ElideMiddle; Layout.fillWidth: true }
                Check {
                    text: qsTr("Can change files")
                    checked: modelData.write
                    onToggled: {
                        var p = JSON.parse(JSON.stringify(sec.s.paths))
                        p[index].write = checked
                        sec.change("paths", p)
                    }
                }
                QQC2.ToolButton {
                    icon.name: "edit-delete"
                    Accessible.name: qsTr("Stop sharing this folder")
                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.text: qsTr("Stop sharing this folder")
                    onClicked: {
                        var p = JSON.parse(JSON.stringify(sec.s.paths))
                        p.splice(index, 1)
                        sec.change("paths", p)
                    }
                }
            }
        }
        QQC2.Button {
            text: qsTr("Share a folder…")
            icon.name: "folder-add"
            onClicked: folderDialog.open()
        }
    }

    // ---- making apps work inside the sandbox
    SectionCard {
        visible: sec.enabledSandbox
        title: qsTr("Compatibility")
        iconName: "tools-wizard"
        Check {
            text: qsTr("Fix Chromium/Electron apps (adds --no-sandbox)")
            checked: !!sec.s.chromium
            onToggled: sec.change("chromium", checked)
        }
        QQC2.Label {
            text: qsTr("Chromium's own sandbox cannot start inside this one; turn this on if the app opens no window.")
            wrapMode: Text.Wrap
            opacity: 0.7
            font: Kirigami.Theme.smallFont
            Layout.fillWidth: true
        }
    }

    FolderDialog {
        id: folderDialog
        title: qsTr("Share a folder with this app")
        onAccepted: {
            var path = decodeURIComponent(selectedFolder.toString().replace(/^file:\/\//, ""))
            var p = JSON.parse(JSON.stringify(sec.s.paths || []))
            for (var i = 0; i < p.length; i++) if (p[i].path === path) return
            p.push({path: path, write: false})
            sec.change("paths", p)
        }
    }
}
