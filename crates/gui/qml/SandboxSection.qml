import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import QtQuick.Dialogs
import org.kde.kirigami as Kirigami

Section {
    id: sec
    property var detail: ({})
    property string appId
    property var backend
    property var s: ({level: "none", paths: []})
    property bool dirty: false

    title: qsTr("Sandbox")
    subtitle: qsTr("Limit what this app can see and do. It runs from an unpacked copy with its own empty home folder.")

    readonly property bool enabledSandbox: s.level !== "none"
    readonly property var levels: ["none", "standard", "strict"]

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

    GridLayout {
        Layout.fillWidth: true
        columns: 2
        columnSpacing: Kirigami.Units.largeSpacing
        rowSpacing: Kirigami.Units.smallSpacing

        QQC2.Label { text: qsTr("Protection:") }
        QQC2.ComboBox {
            id: level
            model: [qsTr("Off"), qsTr("Standard"), qsTr("Strict"), qsTr("Custom")]
            currentIndex: ["none", "standard", "strict", "custom"].indexOf(sec.s.level)
            onActivated: (i) => { if (i < 3) sec.setLevel(sec.levels[i]) }
        }
        Item { width: 1; height: 1 }
        QQC2.Label {
            Layout.fillWidth: true
            Layout.preferredWidth: 1
            wrapMode: Text.Wrap
            opacity: 0.7
            text: sec.s.level === "standard" ? qsTr("Network, graphics, sound and notifications work; the app can't see your files.")
                : sec.s.level === "strict" ? qsTr("No network, no sound, no notifications; the app can't see your files.")
                : sec.s.level === "custom" ? qsTr("Your own mix of the options below.")
                : qsTr("The app runs like any other program and can read your files.")
        }

        QQC2.Label { text: qsTr("Network:") }
        QQC2.Switch { enabled: sec.enabledSandbox; checked: !!sec.s.network; onToggled: sec.change("network", checked) }
        QQC2.Label { text: qsTr("Windows:") }
        QQC2.Switch { enabled: sec.enabledSandbox; checked: !!sec.s.display; onToggled: sec.change("display", checked) }
        QQC2.Label { text: qsTr("Graphics card:") }
        QQC2.Switch { enabled: sec.enabledSandbox; checked: !!sec.s.gpu; onToggled: sec.change("gpu", checked) }
        QQC2.Label { text: qsTr("Sound:") }
        QQC2.Switch { enabled: sec.enabledSandbox; checked: !!sec.s.audio; onToggled: sec.change("audio", checked) }
        QQC2.Label { text: qsTr("Camera:") }
        QQC2.Switch { enabled: sec.enabledSandbox; checked: !!sec.s.devices; onToggled: sec.change("devices", checked) }

        QQC2.Label { text: qsTr("Home folder:") }
        QQC2.ComboBox {
            enabled: sec.enabledSandbox
            readonly property var keys: ["private", "ro", "rw"]
            model: [qsTr("Its own empty folder"), qsTr("Mine, read-only"), qsTr("Mine, read and write")]
            currentIndex: Math.max(0, keys.indexOf(sec.s.home))
            onActivated: (i) => sec.change("home", keys[i])
        }
        QQC2.Label { text: qsTr("Desktop services:") }
        QQC2.ComboBox {
            enabled: sec.enabledSandbox
            readonly property var keys: ["off", "filtered", "full"]
            model: [qsTr("None"), qsTr("File dialogs, notifications, tray icon"), qsTr("Everything (unsafe)")]
            currentIndex: Math.max(0, keys.indexOf(sec.s.bus))
            onActivated: (i) => sec.change("bus", keys[i])
        }
        Item { width: 1; height: 1 }
        QQC2.CheckBox {
            enabled: sec.enabledSandbox
            text: qsTr("Fix Chromium/Electron apps (adds --no-sandbox)")
            checked: !!sec.s.chromium
            onToggled: sec.change("chromium", checked)
        }
    }

    Kirigami.InlineMessage {
        Layout.fillWidth: true
        visible: sec.enabledSandbox && sec.s.home === "rw"
        type: Kirigami.MessageType.Warning
        text: qsTr("With a writable home folder the app can change any of your files.")
    }
    Kirigami.InlineMessage {
        Layout.fillWidth: true
        visible: sec.enabledSandbox && sec.s.bus === "full"
        type: Kirigami.MessageType.Warning
        text: qsTr("With full desktop services the app can talk to every program on your session.")
    }

    QQC2.Label {
        visible: sec.enabledSandbox
        text: qsTr("Folders the app may use")
        font.bold: true
        Layout.topMargin: Kirigami.Units.smallSpacing
    }
    Repeater {
        model: sec.enabledSandbox ? (sec.s.paths || []) : []
        RowLayout {
            required property var modelData
            required property int index
            Layout.fillWidth: true
            QQC2.Label { text: modelData.path; elide: Text.ElideMiddle; Layout.fillWidth: true }
            QQC2.CheckBox {
                text: qsTr("Can change files")
                checked: modelData.write
                onToggled: {
                    var p = JSON.parse(JSON.stringify(sec.s.paths))
                    p[index].write = checked
                    sec.change("paths", p)
                }
            }
            QQC2.Button {
                icon.name: "edit-delete"
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

    RowLayout {
        Layout.topMargin: Kirigami.Units.smallSpacing
        QQC2.Button {
            visible: sec.enabledSandbox
            text: qsTr("Share a folder…")
            icon.name: "folder-add"
            onClicked: folderDialog.open()
        }
        QQC2.Button {
            text: qsTr("Save")
            icon.name: "document-save"
            highlighted: sec.dirty
            enabled: sec.dirty
            onClicked: sec.save()
        }
        QQC2.Button {
            text: qsTr("Revert")
            visible: sec.dirty
            onClicked: { sec.dirty = false; if (sec.detail.sandbox) sec.s = JSON.parse(JSON.stringify(sec.detail.sandbox)) }
        }
    }

    FolderDialog {
        id: folderDialog
        title: qsTr("Share a folder with this app")
        onAccepted: {
            var path = selectedFolder.toString().replace("file://", "")
            var p = JSON.parse(JSON.stringify(sec.s.paths || []))
            for (var i = 0; i < p.length; i++) if (p[i].path === path) return
            p.push({path: path, write: false})
            sec.change("paths", p)
        }
    }
}
