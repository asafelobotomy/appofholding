import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// The "Updates" tab: where new versions come from, what happens when one appears, going back,
// and commands to run around an update.
ColumnLayout {
    id: sec
    property var detail: ({})
    property string appId
    property var backend
    readonly property var src: detail.source || null
    readonly property bool hasUpdate: !!src && src.status === "update-available"
    property var tested: null
    property string testError

    spacing: Kirigami.Units.largeSpacing

    function notify(err) { if (err) applicationWindow().showPassiveNotification(err) }

    // The channel choices; anything else saved is a specific tag.
    readonly property var channelKeys: ["latest", "latest-pre", "latest-all", "tag"]
    function channelLabel(c) {
        switch (c) {
        case "latest": return qsTr("Latest release")
        case "latest-pre": return qsTr("Latest pre-release")
        case "latest-all": return qsTr("Newest of any kind")
        default: return qsTr("Tag “%1”").arg(c)
        }
    }

    // Fill the form with the saved source, unless the user is editing it.
    property bool editing: false
    function fillForm() {
        if (editing) return
        link.text = src ? (src.link || "") : ""
        pattern.text = src ? (src.pattern || "") : ""
        var c = src ? src.channel : "latest"
        var i = channelKeys.indexOf(c)
        channel.currentIndex = i >= 0 && i < 3 ? i : 3
        tag.text = i >= 0 && i < 3 ? "" : c
    }
    onSrcChanged: fillForm()
    onAppIdChanged: { editing = false; tested = null; testError = ""; fillForm() }
    readonly property string chosenChannel: channel.currentIndex === 3 ? tag.text.trim() : channelKeys[channel.currentIndex]

    Connections {
        target: sec.backend
        function onFinished(kind, id, ok, payload) {
            if (kind !== "resolve" || id !== sec.appId) return
            sec.tested = ok ? Util.parse(payload) : null
            sec.testError = ok ? "" : payload
        }
    }

    // ---- status
    SectionCard {
        title: qsTr("Update status")
        iconName: "view-refresh"

        Text {
            visible: !sec.src
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            color: Kirigami.Theme.textColor
            text: qsTr("This app has no update source yet. Paste a GitHub, GitLab or Forgejo link under “Update source” and it will be kept up to date.")
        }
        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: !!sec.src && !!sec.src.error
            type: Kirigami.MessageType.Warning
            text: qsTr("The last check failed: %1").arg(sec.src ? sec.src.error || "" : "")
        }

        GridLayout {
            visible: !!sec.src
            columns: 2
            columnSpacing: Kirigami.Units.gridUnit
            rowSpacing: Kirigami.Units.smallSpacing
            Layout.fillWidth: true

            QQC2.Label { text: qsTr("Status"); opacity: 0.7 }
            Pill {
                readonly property string st: sec.src ? (sec.src.status || "") : ""
                tone: st === "update-available" || st === "up-to-date" ? "positive" : "neutral"
                iconName: st === "update-available" ? "software-update-available"
                    : st === "up-to-date" ? "emblem-ok-symbolic" : "question"
                text: Util.statusLabel(st) || qsTr("Not checked yet")
            }
            QQC2.Label { text: qsTr("Installed"); opacity: 0.7 }
            QQC2.Label { text: sec.src ? (sec.src.installed || qsTr("unknown")) : "" }
            QQC2.Label { text: qsTr("Latest"); opacity: 0.7 }
            QQC2.Label {
                text: !sec.src ? "" : (sec.src.latest || "—")
                    + (sec.src.skipped && sec.src.skipped === sec.src.latest ? qsTr("  (skipped)") : "")
            }
            QQC2.Label { text: qsTr("Last checked"); opacity: 0.7 }
            QQC2.Label { text: sec.src ? Util.ago(sec.src.lastChecked) : "" }
        }

        Flow {
            visible: !!sec.src
            spacing: Kirigami.Units.smallSpacing
            Layout.fillWidth: true

            QQC2.Button {
                text: qsTr("Check now")
                icon.name: "view-refresh"
                enabled: !applicationWindow().working
                onClicked: sec.backend.start("check", sec.appId, "", "", "")
            }
            PrimaryButton {
                text: qsTr("Update to %1").arg(sec.src ? sec.src.latest || "" : "")
                icon.name: "software-update-available"
                visible: sec.hasUpdate
                enabled: !applicationWindow().working
                onClicked: sec.backend.start("update", sec.appId, "", "", "")
            }
            QQC2.Button {
                text: qsTr("Skip this version")
                icon.name: "dialog-cancel"
                visible: sec.hasUpdate && sec.src.skipped !== sec.src.latest
                onClicked: { sec.notify(sec.backend.skip(sec.appId, false)); applicationWindow().reload() }
            }
            QQC2.Button {
                text: qsTr("Stop skipping %1").arg(sec.src ? sec.src.skipped || "" : "")
                icon.name: "edit-undo"
                visible: !!sec.src && !!sec.src.skipped
                onClicked: { sec.notify(sec.backend.skip(sec.appId, true)); applicationWindow().reload() }
            }
        }
    }

    // ---- behaviour
    SectionCard {
        visible: !!sec.src
        title: qsTr("When a new version is found")
        iconName: "preferences-system-time"
        subtitle: qsTr("Used by “Check now”, the background timer and the tray notifications.")

        QQC2.ComboBox {
            id: policy
            readonly property var keys: ["auto", "notify", "manual", "pinned"]
            Layout.fillWidth: true
            Layout.maximumWidth: Kirigami.Units.gridUnit * 24
            model: [
                qsTr("Install it automatically"),
                qsTr("Tell me (default)"),
                qsTr("Stay quiet, I'll update myself"),
                qsTr("Never change this app")
            ]
            currentIndex: sec.src ? Math.max(0, keys.indexOf(sec.src.policy)) : 1
            onActivated: (i) => { sec.notify(sec.backend.setPolicy(sec.appId, keys[i])); applicationWindow().reload() }
        }
    }

    // ---- source
    SectionCard {
        title: qsTr("Update source")
        iconName: "network-server"
        subtitle: !sec.src ? ""
            : sec.src.origin === "embedded" ? qsTr("Found inside the AppImage. Saving a change here replaces it.")
            : qsTr("Currently %1, following %2.").arg(sec.src.describe).arg(sec.channelLabel(sec.src.channel))

        Kirigami.FormLayout {
            Layout.fillWidth: true
            QQC2.TextField {
                id: link
                Kirigami.FormData.label: qsTr("Link:")
                placeholderText: qsTr("https://github.com/owner/repo")
                onTextEdited: sec.editing = true
                Layout.fillWidth: true
                Layout.minimumWidth: Kirigami.Units.gridUnit * 18
            }
            QQC2.TextField {
                id: pattern
                Kirigami.FormData.label: qsTr("File name pattern:")
                placeholderText: qsTr("optional, e.g. Foo-*-x86_64.AppImage")
                onTextEdited: sec.editing = true
                font.family: text.length > 0 ? "monospace" : Kirigami.Theme.defaultFont.family
                Layout.fillWidth: true
                Layout.minimumWidth: Kirigami.Units.gridUnit * 18
            }
            QQC2.ComboBox {
                id: channel
                Kirigami.FormData.label: qsTr("Follow:")
                model: [qsTr("Latest release"), qsTr("Latest pre-release"), qsTr("Newest of any kind"), qsTr("A specific tag…")]
                onActivated: sec.editing = true
            }
            QQC2.TextField {
                id: tag
                visible: channel.currentIndex === 3
                Kirigami.FormData.label: qsTr("Tag:")
                placeholderText: qsTr("e.g. nightly or continuous")
                onTextEdited: sec.editing = true
                Layout.fillWidth: true
                Layout.minimumWidth: Kirigami.Units.gridUnit * 18
            }
        }
        Flow {
            spacing: Kirigami.Units.smallSpacing
            Layout.fillWidth: true
            QQC2.Button {
                text: qsTr("Test")
                icon.name: "dialog-ok-apply"
                enabled: link.text.length > 0 && !applicationWindow().working
                onClicked: { sec.tested = null; sec.testError = ""; sec.backend.start("resolve", sec.appId, link.text, pattern.text, sec.chosenChannel) }
            }
            PrimaryButton {
                text: qsTr("Save")
                icon.name: "document-save"
                enabled: link.text.length > 0 && (sec.editing || !sec.src) && (channel.currentIndex !== 3 || tag.text.trim().length > 0)
                onClicked: {
                    var err = sec.backend.setSource(sec.appId, link.text, pattern.text, sec.chosenChannel)
                    sec.notify(err)
                    if (!err) { sec.editing = false; sec.tested = null; applicationWindow().reload() }
                }
            }
            QQC2.Button {
                text: qsTr("Revert")
                icon.name: "edit-undo"
                visible: sec.editing
                onClicked: { sec.editing = false; sec.tested = null; sec.testError = ""; sec.fillForm() }
            }
            QQC2.Button {
                text: qsTr("Remove source")
                icon.name: "edit-clear"
                visible: !!sec.src && !sec.editing
                onClicked: { sec.notify(sec.backend.clearSource(sec.appId)); applicationWindow().reload() }
            }
        }
        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: sec.testError.length > 0
            type: Kirigami.MessageType.Error
            text: sec.testError
        }
        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: !!sec.tested
            type: sec.tested && sec.tested.problem ? Kirigami.MessageType.Warning : Kirigami.MessageType.Positive
            text: !sec.tested ? "" : (sec.tested.problem
                  ? sec.tested.problem
                  : qsTr("Found %1 — it would download %2.").arg(sec.tested.release).arg(sec.tested.chosen))
        }
    }

    // ---- previous versions
    SectionCard {
        visible: !!sec.detail.versions && sec.detail.versions.length > 0
        title: qsTr("Previous versions")
        iconName: "edit-undo"
        subtitle: qsTr("Kept so you can go back if a new version misbehaves.")

        Repeater {
            model: sec.detail.versions || []
            RowLayout {
                required property var modelData
                Layout.fillWidth: true
                QQC2.Label { text: modelData.version || modelData.sha; Layout.fillWidth: true; elide: Text.ElideRight }
                QQC2.Button {
                    text: qsTr("Roll back")
                    icon.name: "edit-undo"
                    onClicked: { sec.notify(sec.backend.rollback(sec.appId, modelData.key)); applicationWindow().reload() }
                }
            }
        }
    }

    // ---- hooks
    SectionCard {
        title: qsTr("Commands around updates")
        iconName: "system-run"
        subtitle: qsTr("Optional shell commands, saved when you leave the field. A failing “before” command cancels the update.")

        Kirigami.FormLayout {
            Layout.fillWidth: true
            Repeater {
                model: [
                    {kind: "pre-update", label: qsTr("Before updating:")},
                    {kind: "post-update", label: qsTr("After updating:")}
                ]
                QQC2.TextField {
                    required property var modelData
                    Kirigami.FormData.label: modelData.label
                    Layout.fillWidth: true
                    Layout.minimumWidth: Kirigami.Units.gridUnit * 18
                    font.family: "monospace"
                    // Set by hand, not bound: a refresh while typing must not replace the text.
                    readonly property string saved: {
                        var hs = sec.detail.hooks || []
                        for (var i = 0; i < hs.length; i++) if (hs[i].kind === modelData.kind) return hs[i].command
                        return ""
                    }
                    onSavedChanged: if (!activeFocus) text = saved
                    Component.onCompleted: text = saved
                    onEditingFinished: if (text !== saved) {
                        var err = sec.backend.setHook(sec.appId, modelData.kind, text)
                        sec.notify(err)
                        if (!err) {
                            applicationWindow().showPassiveNotification(qsTr("Command saved"))
                            applicationWindow().reload()
                        }
                    }
                }
            }
        }
    }
}
