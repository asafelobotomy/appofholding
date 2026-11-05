import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// The "Launch" tab.
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
        dirty = false
        loadedFor = appId
        var r = Util.parse(backend.profileDetail(appId, name))
        form = r.error ? ({}) : r
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

    onDetailChanged: if (detail.id) {
        presets = Util.parse(backend.presets())
        var keep = detail.profiles && detail.profiles.indexOf(profileName) >= 0
        // Keep unsaved edits when the same app's details are merely refreshed.
        if (dirty && keep && loadedFor === appId) return
        loadProfile(keep ? profileName : "default")
    }

    // ---- profile + quick fixes
    SectionCard {
        title: qsTr("Quick fixes")
        iconName: "tools-wizard"
        subtitle: qsTr("Ready-made settings for common problems (Wayland, blank windows, sandbox errors…). Hover one to see what it does.")

        RowLayout {
            spacing: Kirigami.Units.smallSpacing
            QQC2.Label { text: qsTr("Profile:") }
            QQC2.ComboBox {
                id: profiles
                model: sec.detail.profiles || []
                currentIndex: Math.max(0, (sec.detail.profiles || []).indexOf(sec.profileName))
                onActivated: (i) => sec.loadProfile(model[i])
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
        QQC2.Label {
            visible: sec.profileName !== "default"
            text: qsTr("This profile appears in the app's right-click menu.")
            opacity: 0.7
        }

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
        title: qsTr("Arguments and environment")
        iconName: "utilities-terminal"

        Kirigami.FormLayout {
            Layout.fillWidth: true
            QQC2.TextField {
                id: args
                Kirigami.FormData.label: qsTr("Extra arguments:")
                text: sec.form.args || ""
                placeholderText: qsTr("--flag \"value with spaces\"")
                onTextEdited: sec.dirty = true
                Layout.fillWidth: true
                Layout.minimumWidth: Kirigami.Units.gridUnit * 18
            }
            QQC2.TextArea {
                id: env
                Kirigami.FormData.label: qsTr("Environment:")
                text: sec.form.env || ""
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
                text: sec.form.prefix || ""
                placeholderText: qsTr("e.g. gamemoderun")
                onTextEdited: sec.dirty = true
                Layout.fillWidth: true
                Layout.minimumWidth: Kirigami.Units.gridUnit * 18
            }
            QQC2.TextField {
                id: cwd
                Kirigami.FormData.label: qsTr("Working folder:")
                text: sec.form.cwd || ""
                onTextEdited: sec.dirty = true
                Layout.fillWidth: true
                Layout.minimumWidth: Kirigami.Units.gridUnit * 18
            }
            QQC2.CheckBox {
                id: extract
                text: qsTr("Unpack instead of mounting (no FUSE needed)")
                checked: !!sec.form.extractAndRun
                onToggled: sec.dirty = true
            }
            QQC2.CheckBox {
                id: elevated
                text: qsTr("Run as administrator (root)")
                checked: !!sec.form.elevated
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
                onClicked: {
                    var err = sec.backend.saveProfile(sec.appId, sec.profileName, JSON.stringify(sec.collect()))
                    sec.notify(err)
                    if (!err) {
                        sec.dirty = false
                        applicationWindow().showPassiveNotification(qsTr("Launch settings saved"))
                        applicationWindow().reload()   // the Overview summarises these settings
                    }
                }
            }
        }
    }

    // ---- preview
    SectionCard {
        title: qsTr("What will run")
        iconName: "view-preview"
        subtitle: qsTr("The exact command for the saved settings.")

        QQC2.TextArea {
            readonly property bool long: sec.previewText.length > 260
            text: long && !sec.showFull ? sec.previewText.substring(0, 220).replace(/\s+\S*$/, "") + " …" : sec.previewText
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
                text: qsTr("Refresh")
                icon.name: "view-refresh"
                onClicked: sec.refreshPreview()
            }
        }
    }

    // ---- hooks
    SectionCard {
        title: qsTr("Commands around updates")
        iconName: "system-run"
        subtitle: qsTr("Shell commands run when this app is updated. A failing “before update” command cancels the update.")

        Kirigami.FormLayout {
            Layout.fillWidth: true
            Repeater {
                model: [
                    {kind: "pre-update", label: qsTr("Before update:")},
                    {kind: "post-update", label: qsTr("After update:")}
                ]
                QQC2.TextField {
                    required property var modelData
                    Kirigami.FormData.label: modelData.label
                    Layout.fillWidth: true
                    Layout.minimumWidth: Kirigami.Units.gridUnit * 18
                    // Set by hand, not bound: a refresh while typing must not replace the text.
                    readonly property string saved: {
                        var hs = sec.detail.hooks || []
                        for (var i = 0; i < hs.length; i++) if (hs[i].kind === modelData.kind) return hs[i].command
                        return ""
                    }
                    onSavedChanged: if (!activeFocus) text = saved
                    Component.onCompleted: text = saved
                    onEditingFinished: if (text !== saved) sec.notify(sec.backend.setHook(sec.appId, modelData.kind, text))
                }
            }
        }
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
        }
        onAccepted: {
            var err = sec.backend.saveProfile(sec.appId, newName.text, JSON.stringify(sec.collect()))
            sec.notify(err)
            if (!err) { sec.profileName = newName.text; applicationWindow().reload() }
            newName.text = ""
        }
    }
}
