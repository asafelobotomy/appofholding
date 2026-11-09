import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// The "Launch" tab: how the app is started, per launch profile.
ColumnLayout {
    id: sec
    property var detail: ({})
    property string appId
    property var backend
    property var form: ({})
    property var presets: []
    property string profileName: "default"
    property string previewText
    property bool showFull: false
    // Edits not saved yet. A reload (another app's update, a check finishing) must not wipe them.
    property bool dirty: false
    property string loadedFor

    spacing: Kirigami.Units.largeSpacing

    function notify(err) { if (err) applicationWindow().showPassiveNotification(err) }

    function loadProfile(name) {
        profileName = name
        loadedFor = appId
        var r = Util.parse(backend.profileDetail(appId, name))
        form = r.error ? ({}) : r
        // The fields are set by hand rather than bound, so typing never fights a reload.
        args.text = form.args || ""
        env.text = form.env || ""
        prefix.text = form.prefix || ""
        cwd.text = form.cwd || ""
        extract.checked = !!form.extractAndRun
        elevated.checked = !!form.elevated
        for (var i = 0; i < presetRepeater.count; i++) {
            var chip = presetRepeater.itemAt(i)
            if (chip) chip.checked = (form.presets || []).indexOf(chip.key) >= 0
        }
        dirty = false   // last: setting the environment text above can mark it changed
        refreshPreview()
    }

    function refreshPreview() {
        var r = Util.parse(backend.preview(appId, profileName))
        previewText = r.error ? r.error : r.text
    }

    function collect() {
        return {
            args: args.text, env: env.text, prefix: prefix.text, cwd: cwd.text,
            presets: chosenPresets(), extractAndRun: extract.checked, elevated: elevated.checked
        }
    }

    function chosenPresets() {
        var out = []
        for (var i = 0; i < presetRepeater.count; i++) {
            var item = presetRepeater.itemAt(i)
            if (item && item.checked) out.push(item.key)
        }
        return out
    }

    function save() {
        var err = backend.saveProfile(appId, profileName, JSON.stringify(collect()))
        notify(err)
        if (err) return false
        dirty = false
        applicationWindow().showPassiveNotification(qsTr("Launch settings saved"))
        applicationWindow().reload()   // the Overview summarises these settings
        return true
    }

    function revert() { loadProfile(profileName) }

    onDetailChanged: if (detail.id) {
        presets = Util.parse(backend.presets())
        var keep = detail.profiles && detail.profiles.indexOf(profileName) >= 0
        // Keep unsaved edits when the same app's details are merely refreshed.
        if (dirty && keep && loadedFor === appId) return
        loadProfile(keep ? profileName : "default")
    }

    // ---- which profile
    SectionCard {
        title: qsTr("Profile")
        iconName: "view-list-details"
        subtitle: sec.profileName === "default"
            ? qsTr("The default profile is used when you open the app. Extra profiles appear in its right-click menu.")
            : qsTr("This profile appears in the app's right-click menu.")

        RowLayout {
            spacing: Kirigami.Units.smallSpacing
            QQC2.ComboBox {
                id: profiles
                model: sec.detail.profiles || []
                currentIndex: Math.max(0, (sec.detail.profiles || []).indexOf(sec.profileName))
                onActivated: (i) => sec.loadProfile(model[i])
                Accessible.name: qsTr("Profile")
            }
            QQC2.Button {
                text: qsTr("New…")
                icon.name: "list-add"
                onClicked: newProfile.open()
            }
            QQC2.Button {
                text: qsTr("Delete")
                icon.name: "edit-delete"
                visible: sec.profileName !== "default"
                onClicked: {
                    var err = sec.backend.deleteProfile(sec.appId, sec.profileName)
                    sec.notify(err)
                    if (err) return
                    sec.dirty = false
                    sec.profileName = "default"
                    applicationWindow().reload()
                }
            }
        }
    }

    // ---- quick fixes
    SectionCard {
        title: qsTr("Quick fixes")
        iconName: "tools-wizard"
        subtitle: qsTr("Ready-made settings for common problems (Wayland, blank windows, sandbox errors…). Hover one to see what it does.")

        Flow {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing
            Repeater {
                id: presetRepeater
                model: sec.presets
                Kirigami.Chip {
                    required property var modelData
                    readonly property string key: modelData.name
                    text: modelData.name
                    checkable: true
                    closable: false
                    checked: (sec.form.presets || []).indexOf(modelData.name) >= 0
                    onToggled: sec.dirty = true
                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.text: modelData.description
                    QQC2.ToolTip.delay: Kirigami.Units.toolTipDelay
                }
            }
        }
    }

    // ---- arguments, environment
    SectionCard {
        title: qsTr("Options")
        iconName: "utilities-terminal"

        Kirigami.FormLayout {
            Layout.fillWidth: true
            QQC2.TextField {
                id: args
                Kirigami.FormData.label: qsTr("Extra arguments:")
                placeholderText: qsTr("--flag \"value with spaces\"")
                onTextEdited: sec.dirty = true
                font.family: "monospace"
                Layout.fillWidth: true
                Layout.minimumWidth: Kirigami.Units.gridUnit * 18
            }
            QQC2.TextArea {
                id: env
                Kirigami.FormData.label: qsTr("Environment:")
                placeholderText: qsTr("NAME=value, one per line")
                onTextChanged: if (activeFocus) sec.dirty = true
                font.family: "monospace"
                Layout.fillWidth: true
                Layout.minimumWidth: Kirigami.Units.gridUnit * 18
                Layout.preferredHeight: Kirigami.Units.gridUnit * 4
            }
            QQC2.TextField {
                id: prefix
                Kirigami.FormData.label: qsTr("Run under:")
                placeholderText: qsTr("optional, e.g. gamemoderun")
                onTextEdited: sec.dirty = true
                font.family: "monospace"
                Layout.fillWidth: true
                Layout.minimumWidth: Kirigami.Units.gridUnit * 18
            }
            QQC2.TextField {
                id: cwd
                Kirigami.FormData.label: qsTr("Working folder:")
                placeholderText: qsTr("optional, e.g. ~/Documents")
                onTextEdited: sec.dirty = true
                Layout.fillWidth: true
                Layout.minimumWidth: Kirigami.Units.gridUnit * 18
            }
            QQC2.CheckBox {
                id: extract
                text: qsTr("Unpack instead of mounting (no FUSE needed)")
                onToggled: sec.dirty = true
            }
            QQC2.CheckBox {
                id: elevated
                text: qsTr("Run as administrator (root)")
                onToggled: sec.dirty = true
            }
        }
        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: elevated.checked
            type: Kirigami.MessageType.Warning
            text: qsTr("The app gets full control of your system. It cannot be combined with a sandbox.")
        }
        RowLayout {
            PrimaryButton {
                text: qsTr("Save")
                icon.name: "document-save"
                enabled: sec.dirty
                onClicked: sec.save()
            }
            QQC2.Button {
                text: qsTr("Revert")
                icon.name: "edit-undo"
                enabled: sec.dirty
                onClicked: sec.revert()
            }
        }
    }

    // ---- preview
    SectionCard {
        title: qsTr("What will run")
        iconName: "view-preview"
        subtitle: sec.dirty ? qsTr("The command for the saved settings; save to see your changes here.")
                            : qsTr("The exact command for this profile, sandbox included.")

        QQC2.TextArea {
            readonly property bool isLong: sec.previewText.length > 260
            text: isLong && !sec.showFull ? sec.previewText.substring(0, 220).replace(/\s+\S*$/, "") + " …" : sec.previewText
            readOnly: true
            wrapMode: Text.WrapAnywhere
            font.family: "monospace"
            Layout.fillWidth: true
            background: Rectangle { color: Kirigami.Theme.alternateBackgroundColor; radius: Kirigami.Units.smallSpacing }
        }
        RowLayout {
            QQC2.Button {
                visible: sec.previewText.length > 260
                flat: true
                text: sec.showFull ? qsTr("Show less") : qsTr("Show the full command")
                icon.name: sec.showFull ? "arrow-up" : "arrow-down"
                onClicked: sec.showFull = !sec.showFull
            }
            QQC2.Button {
                flat: true
                text: qsTr("Copy")
                icon.name: "edit-copy"
                onClicked: {
                    clip.text = sec.previewText
                    clip.selectAll()
                    clip.copy()
                    applicationWindow().showPassiveNotification(qsTr("Command copied"))
                }
            }
        }
        QQC2.TextField { id: clip; visible: false }
    }

    QQC2.Dialog {
        id: newProfile
        title: qsTr("New launch profile")
        modal: true
        anchors.centerIn: QQC2.Overlay.overlay
        standardButtons: QQC2.Dialog.Ok | QQC2.Dialog.Cancel
        contentItem: ColumnLayout {
            QQC2.Label { text: qsTr("Name (lowercase letters, digits and dashes):") }
            QQC2.TextField { id: newName; Layout.fillWidth: true }
            QQC2.Label {
                text: qsTr("It starts as a copy of the options shown now.")
                opacity: 0.7
                font: Kirigami.Theme.smallFont
            }
        }
        onAccepted: {
            var err = sec.backend.saveProfile(sec.appId, newName.text, JSON.stringify(sec.collect()))
            sec.notify(err)
            if (!err) { sec.dirty = false; sec.profileName = newName.text; applicationWindow().reload() }
            newName.text = ""
        }
    }
}
