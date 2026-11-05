import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Kirigami.Dialog {
    id: dialog
    title: qsTr("Add from a link")
    preferredWidth: Kirigami.Units.gridUnit * 30
    padding: Kirigami.Units.largeSpacing
    standardButtons: Kirigami.Dialog.NoButton

    property var resolved: null
    property string error
    property bool installing: false
    property bool resolving: false
    // What the shown preview was looked up with; installing always uses exactly this.
    property string lookedUpLink
    property string lookedUpPattern
    readonly property var backend: applicationWindow().backendObject
    readonly property real downloading: applicationWindow().progress["@install"] !== undefined ? applicationWindow().progress["@install"] : -2

    function openDialog() {
        resolved = null
        error = ""
        installing = false
        resolving = false
        link.text = ""
        pattern.text = ""
        open()
        link.forceActiveFocus()
    }

    function resolve() {
        if (resolving || installing || link.text.length === 0) return
        resolved = null
        error = ""
        resolving = true
        lookedUpLink = link.text
        lookedUpPattern = pattern.text
        backend.start("resolve", "", link.text, pattern.text, "latest")
    }

    function install() {
        if (installing || !resolved) return
        installing = true
        error = ""
        backend.start("install", "", lookedUpLink, lookedUpPattern, "latest")
    }

    // Editing the link or pattern makes the preview out of date: look it up again.
    function inputChanged() {
        if (resolved && (link.text !== lookedUpLink || pattern.text !== lookedUpPattern)) resolved = null
    }

    Connections {
        target: dialog.backend
        function onFinished(kind, id, ok, payload) {
            if (id !== "") return
            if (kind === "resolve") dialog.resolving = false
            if (kind === "install") dialog.installing = false
            if (!dialog.visible) return
            if (kind === "resolve") {
                dialog.resolving = false
                dialog.resolved = ok ? Util.parse(payload) : null
                dialog.error = ok ? "" : payload
            } else if (kind === "install") {
                dialog.installing = false
                if (ok) dialog.close()
                else dialog.error = payload
            }
        }
    }

    contentItem: ColumnLayout {
        spacing: Kirigami.Units.largeSpacing

        QQC2.Label {
            text: qsTr("Paste the address of a GitHub, GitLab or Forgejo project, a release page, or a direct download link.")
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
        QQC2.TextField {
            id: link
            placeholderText: qsTr("https://github.com/owner/repo")
            Layout.fillWidth: true
            enabled: !dialog.installing
            onAccepted: dialog.resolve()
            onTextChanged: dialog.inputChanged()
        }
        QQC2.TextField {
            id: pattern
            placeholderText: qsTr("File name pattern (only if several files match)")
            Layout.fillWidth: true
            enabled: !dialog.installing
            onAccepted: dialog.resolve()
            onTextChanged: dialog.inputChanged()
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: dialog.error.length > 0
            type: Kirigami.MessageType.Error
            text: dialog.error
        }

        ColumnLayout {
            visible: !!dialog.resolved
            Layout.fillWidth: true
            Kirigami.InlineMessage {
                Layout.fillWidth: true
                visible: !!dialog.resolved
                type: dialog.resolved && dialog.resolved.problem ? Kirigami.MessageType.Warning : Kirigami.MessageType.Positive
                text: !dialog.resolved ? "" : (dialog.resolved.problem
                      ? dialog.resolved.problem
                      : qsTr("%1 %2 — will download %3").arg(dialog.resolved.source).arg(dialog.resolved.release).arg(dialog.resolved.chosen))
            }
            QQC2.Label {
                visible: !!dialog.resolved && dialog.resolved.candidates.length > 1
                text: qsTr("Several files match. Type part of the name you want in the pattern box above, for example:")
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
            Repeater {
                model: dialog.resolved && dialog.resolved.candidates.length > 1 ? dialog.resolved.candidates : []
                QQC2.Button {
                    required property string modelData
                    text: modelData
                    flat: true
                    onClicked: pattern.text = modelData
                }
            }
        }

        QQC2.ProgressBar {
            visible: dialog.installing
            indeterminate: dialog.downloading < 0
            value: Math.max(0, dialog.downloading)
            Layout.fillWidth: true
        }

        RowLayout {
            Layout.alignment: Qt.AlignRight
            QQC2.Button {
                text: qsTr("Look up")
                icon.name: "edit-find"
                enabled: link.text.length > 0 && !dialog.installing && !dialog.resolving
                onClicked: dialog.resolve()
            }
            PrimaryButton {
                text: qsTr("Download and add")
                icon.name: "download"
                enabled: !!dialog.resolved && !dialog.resolved.problem && !dialog.installing
                onClicked: dialog.install()
            }
        }
    }
}
