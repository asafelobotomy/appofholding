import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// The "Updates" tab.
ColumnLayout {
    id: sec
    property var detail: ({})
    property string appId
    property var backend
    readonly property var src: detail.source || null
    readonly property real downloading: applicationWindow().progress[appId] !== undefined ? applicationWindow().progress[appId] : -2
    property var tested: null
    property string testError

    spacing: Kirigami.Units.largeSpacing

    function notify(err) { if (err) applicationWindow().showPassiveNotification(err) }

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
            text: qsTr("This app has no update source yet. Paste a GitHub, GitLab or Forgejo link below and it will be kept up to date.")
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
                tone: !sec.src ? "neutral" : sec.src.status === "update-available" ? "positive" : sec.src.status === "up-to-date" ? "positive" : "neutral"
                iconName: sec.src && sec.src.status === "update-available" ? "software-update-available" : "emblem-ok-symbolic"
                text: sec.src ? (Util.statusLabel(sec.src.status) || qsTr("Not checked yet")) : ""
            }
            QQC2.Label { text: qsTr("Installed"); opacity: 0.7 }
            QQC2.Label { text: sec.src ? (sec.src.installed || qsTr("unknown")) : "" }
            QQC2.Label { text: qsTr("Latest"); opacity: 0.7 }
            QQC2.Label { text: sec.src ? (sec.src.latest || "—") : "" }
            QQC2.Label { text: qsTr("Source"); opacity: 0.7 }
            QQC2.Label {
                text: sec.src ? sec.src.describe + (sec.src.origin === "embedded" ? qsTr("  (found inside the AppImage)") : "") : ""
                wrapMode: Text.WrapAnywhere
                Layout.fillWidth: true
            }
            QQC2.Label { text: qsTr("Last checked"); opacity: 0.7 }
            QQC2.Label { text: sec.src ? Util.ago(sec.src.lastChecked) : "" }
            QQC2.Label { text: qsTr("File pattern"); opacity: 0.7; visible: !!(sec.src && sec.src.pattern) }
            QQC2.Label { text: sec.src ? (sec.src.pattern || "") : ""; visible: !!(sec.src && sec.src.pattern); font.family: "monospace" }
        }

        QQC2.ProgressBar {
            visible: sec.downloading > -2
            indeterminate: sec.downloading < 0
            value: Math.max(0, sec.downloading)
            Layout.fillWidth: true
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
                text: qsTr("Update")
                icon.name: "software-update-available"
                visible: !!sec.src && sec.src.status === "update-available"
                enabled: !applicationWindow().working
                onClicked: sec.backend.start("update", sec.appId, "", "", "")
            }
            QQC2.Button {
                text: qsTr("Skip this version")
                icon.name: "dialog-cancel"
                visible: !!sec.src && sec.src.status === "update-available"
                onClicked: { sec.notify(sec.backend.skip(sec.appId, false)); applicationWindow().reload() }
            }
            QQC2.Button {
                text: qsTr("Stop skipping %1").arg(sec.src ? sec.src.skipped || "" : "")
                visible: !!sec.src && !!sec.src.skipped
                onClicked: { sec.notify(sec.backend.skip(sec.appId, true)); applicationWindow().reload() }
            }
        }
    }

    // ---- behaviour
    SectionCard {
        visible: !!sec.src
        title: qsTr("When an update is found")
        iconName: "preferences-system-time"
        subtitle: qsTr("Applies to the background checks too.")

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

    // ---- previous versions
    SectionCard {
        visible: !!sec.src && !!sec.detail.versions && sec.detail.versions.length > 0
        title: qsTr("Previous versions")
        iconName: "edit-undo"
        subtitle: qsTr("Kept so you can go back if a new version misbehaves.")

        Repeater {
            model: sec.detail.versions || []
            RowLayout {
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

    // ---- change source
    SectionCard {
        title: sec.src ? qsTr("Change the update source") : qsTr("Set an update source")
        iconName: "network-server"

        Kirigami.FormLayout {
            Layout.fillWidth: true
            QQC2.TextField {
                id: link
                Kirigami.FormData.label: qsTr("Link:")
                placeholderText: qsTr("https://github.com/owner/repo")
                Layout.fillWidth: true
                Layout.minimumWidth: Kirigami.Units.gridUnit * 18
            }
            QQC2.TextField {
                id: pattern
                Kirigami.FormData.label: qsTr("File name pattern:")
                placeholderText: qsTr("optional, e.g. Foo-*-x86_64.AppImage")
                Layout.fillWidth: true
                Layout.minimumWidth: Kirigami.Units.gridUnit * 18
            }
            QQC2.ComboBox {
                id: channel
                Kirigami.FormData.label: qsTr("Follow:")
                readonly property var keys: ["latest", "latest-pre", "latest-all"]
                model: [qsTr("Latest release"), qsTr("Latest pre-release"), qsTr("Newest of any kind")]
            }
        }
        Flow {
            spacing: Kirigami.Units.smallSpacing
            Layout.fillWidth: true
            QQC2.Button {
                text: qsTr("Test")
                icon.name: "dialog-ok-apply"
                enabled: link.text.length > 0 && !applicationWindow().working
                onClicked: { sec.tested = null; sec.testError = ""; sec.backend.start("resolve", sec.appId, link.text, pattern.text, channel.keys[channel.currentIndex]) }
            }
            PrimaryButton {
                text: qsTr("Save")
                icon.name: "document-save"
                enabled: link.text.length > 0
                onClicked: {
                    var err = sec.backend.setSource(sec.appId, link.text, pattern.text, channel.keys[channel.currentIndex])
                    sec.notify(err)
                    if (!err) { link.text = ""; pattern.text = ""; applicationWindow().reload() }
                }
            }
            QQC2.Button {
                text: qsTr("Remove source")
                icon.name: "edit-clear"
                visible: !!sec.src
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
}
