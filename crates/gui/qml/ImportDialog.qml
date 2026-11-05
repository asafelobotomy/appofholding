import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// Import AppImages that already exist on this computer.
// 1. scanning  2. plan (a dry run: nothing has changed)  3. final confirmation  4. result
Kirigami.Dialog {
    id: dialog
    title: qsTr("Import existing AppImages")
    preferredWidth: Kirigami.Units.gridUnit * 42
    preferredHeight: Kirigami.Units.gridUnit * 32
    padding: Kirigami.Units.largeSpacing
    standardButtons: Kirigami.Dialog.NoButton

    readonly property var backend: applicationWindow().backendObject
    property string phase: "scan"          // scan | plan | confirm | run | done
    property var plan: ({items: [], searched: [], skipped: []})
    property var selected: ({})
    property var results: []
    property string error
    property bool replacedAny: false
    readonly property var modes: ["default", "in-place", "copy", "move"]

    readonly property var chosen: {
        var out = []
        for (var i = 0; i < plan.items.length; i++) if (selected[plan.items[i].path]) out.push(plan.items[i])
        return out
    }
    readonly property int movedCount: chosen.filter(function (i) { return i.mode === "move" }).length
    readonly property int copiedCount: chosen.filter(function (i) { return i.mode === "copy" }).length
    readonly property int staysCount: chosen.length - movedCount - copiedCount
    readonly property int replacesCount: chosen.reduce(function (n, i) { return n + i.replaces }, 0)
    readonly property int twiceCount: chosen.filter(function (i) { return (i.owner === "tool" || i.owner === "entry") && i.replaces === 0 }).length

    function plural(n, one, other) { return (n === 1 ? one : other).arg(n) }
    function setAll(value) {
        var s = ({})
        for (var i = 0; i < plan.items.length; i++) s[plan.items[i].path] = value && plan.items[i].action !== "skip"
        selected = s
    }

    function options() {
        return JSON.stringify({mode: modes[modeBox.currentIndex], replaceEntries: replaceBox.checked})
    }

    function openDialog() {
        phase = "scan"
        error = ""
        results = []
        selected = ({})
        replacedAny = false
        modeBox.currentIndex = 0
        replaceBox.checked = false
        open()
        scan()
    }

    function scan() {
        var keep = Object.assign({}, selected)
        dialog.keep = keep
        phase = "scan"
        backend.start("importScan", "", options(), "", "")
    }
    property var keep: ({})

    Connections {
        target: dialog.backend
        function onFinished(kind, id, ok, payload) {
            if (id !== "" || !dialog.visible) return
            if (kind === "importScan") {
                if (!ok) { dialog.error = payload; dialog.phase = "plan"; return }
                dialog.error = ""
                dialog.plan = Util.parse(payload)
                var sel = ({})
                var first = Object.keys(dialog.keep).length === 0
                for (var i = 0; i < dialog.plan.items.length; i++) {
                    var it = dialog.plan.items[i]
                    // The first plan selects everything that can be imported as is; later plans
                    // (after changing an option) keep what the user chose.
                    sel[it.path] = it.action !== "skip" && (first ? it.action === "import" : !!dialog.keep[it.path])
                }
                dialog.selected = sel
                dialog.phase = "plan"
            } else if (kind === "importRun") {
                if (!ok) { dialog.error = payload; dialog.phase = "plan"; return }
                var r = Util.parse(payload)
                dialog.results = r.results
                dialog.replacedAny = dialog.replacesCount > 0
                dialog.phase = "done"
                applicationWindow().reload()
            }
        }
    }

    function setSelected(path, value) {
        var s = Object.assign({}, selected)
        s[path] = value
        selected = s
    }

    function run() {
        phase = "run"
        var paths = chosen.map(function (i) { return i.path })
        backend.start("importRun", "", options(), JSON.stringify(paths), "")
    }

    // /home/me/AppImages -> ~/AppImages
    function tilde(p) {
        var h = plan.home
        return h && p.indexOf(h + "/") === 0 ? "~" + p.substring(h.length) : p
    }

    function ownerText(i) {
        switch (i.owner) {
        case "tool": return qsTr("Managed by %1").arg(i.tool)
        case "entry": return qsTr("Has its own menu entry")
        case "app": return qsTr("Kept by the app itself")
        default: return qsTr("Loose file")
        }
    }

    contentItem: ColumnLayout {
        spacing: Kirigami.Units.largeSpacing

        // ---- 1. scanning
        ColumnLayout {
            visible: dialog.phase === "scan" || dialog.phase === "run"
            Layout.fillWidth: true
            Layout.fillHeight: true
            Item { Layout.fillHeight: true }
            QQC2.BusyIndicator { running: true; Layout.alignment: Qt.AlignHCenter }
            QQC2.Label {
                text: dialog.phase === "run" ? qsTr("Importing…") : qsTr("Looking for AppImages…")
                Layout.alignment: Qt.AlignHCenter
            }
            Item { Layout.fillHeight: true }
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: dialog.error.length > 0
            type: Kirigami.MessageType.Error
            text: dialog.error
        }

        // ---- 2. the plan (dry run)
        ColumnLayout {
            visible: dialog.phase === "plan"
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: Kirigami.Units.smallSpacing

            // Nothing to import
            ColumnLayout {
                visible: dialog.plan.items.length === 0
                Layout.fillWidth: true
                Layout.fillHeight: true
                Item { Layout.fillHeight: true }
                Kirigami.PlaceholderMessage {
                    Layout.fillWidth: true
                    icon.name: "checkmark"
                    text: qsTr("Nothing to import")
                    explanation: qsTr("No AppImages that are not already in your library were found in %1.").arg(dialog.plan.searched.map(dialog.tilde).join(", "))
                }
                Item { Layout.fillHeight: true }
            }

            QQC2.Label {
                visible: dialog.plan.items.length > 0
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                text: dialog.plural(dialog.plan.items.length, qsTr("Found %1 AppImage in %2."), qsTr("Found %1 AppImages in %2.")).arg(dialog.plan.searched.map(dialog.tilde).join(", "))
                    + " " + qsTr("Nothing has been changed yet — choose what to import, then confirm.")
            }

            RowLayout {
                visible: dialog.plan.items.length > 0
                spacing: Kirigami.Units.largeSpacing
                QQC2.Label { text: qsTr("Files:") }
                QQC2.ComboBox {
                    id: modeBox
                    implicitContentWidthPolicy: QQC2.ComboBox.WidestText
                    model: [qsTr("Smart (Downloads are moved, others stay)"), qsTr("Leave where they are"), qsTr("Copy into the library"), qsTr("Move into the library")]
                    onActivated: dialog.scan()
                }
                QQC2.CheckBox {
                    id: replaceBox
                    text: qsTr("Back up other tools' menu entries")
                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.text: qsTr("Gear Lever and AppImageLauncher entries are moved to a backup so each app appears once. You can restore them.")
                    onToggled: dialog.scan()
                }
            }

            RowLayout {
                visible: dialog.plan.items.length > 0
                QQC2.Label { text: qsTr("%1 selected").arg(dialog.chosen.length); opacity: 0.7 }
                Item { Layout.fillWidth: true }
                QQC2.Button { flat: true; text: qsTr("Select all"); onClicked: dialog.setAll(true) }
                QQC2.Button { flat: true; text: qsTr("Select none"); onClicked: dialog.setAll(false) }
            }

            QQC2.ScrollView {
                visible: dialog.plan.items.length > 0
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                ListView {
                    model: dialog.plan.items
                    spacing: Kirigami.Units.smallSpacing
                    delegate: Rectangle {
                        id: row
                        required property var modelData
                        width: ListView.view ? ListView.view.width - Kirigami.Units.gridUnit : 0
                        implicitHeight: rowColumn.implicitHeight + Kirigami.Units.smallSpacing * 2
                        height: implicitHeight
                        radius: Kirigami.Units.cornerRadius
                        color: Kirigami.ColorUtils.tintWithAlpha(Kirigami.Theme.backgroundColor, Kirigami.Theme.textColor, 0.045)
                        border.width: 1
                        border.color: !!dialog.selected[modelData.path]
                            ? Kirigami.ColorUtils.tintWithAlpha(Kirigami.Theme.backgroundColor, Kirigami.Theme.highlightColor, 0.7)
                            : Kirigami.ColorUtils.tintWithAlpha(Kirigami.Theme.backgroundColor, Kirigami.Theme.textColor, 0.12)
                        ColumnLayout {
                        id: rowColumn
                        anchors { left: parent.left; right: parent.right; top: parent.top; margins: Kirigami.Units.smallSpacing }
                        spacing: 0
                        RowLayout {
                            Layout.fillWidth: true
                            QQC2.CheckBox {
                                enabled: modelData.action !== "skip"
                                checked: !!dialog.selected[modelData.path]
                                onToggled: dialog.setSelected(modelData.path, checked)
                            }
                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 0
                                QQC2.Label {
                                    text: modelData.name + (modelData.version ? "  " + modelData.version : "")
                                    font.bold: true
                                    elide: Text.ElideRight
                                    Layout.fillWidth: true
                                }
                                QQC2.Label {
                                    text: dialog.ownerText(modelData) + "  ·  " + dialog.tilde(modelData.path)
                                    opacity: 0.7
                                    elide: Text.ElideMiddle
                                    Layout.fillWidth: true
                                }
                            }
                        }
                        Repeater {
                            model: modelData.notes
                            QQC2.Label {
                                required property string modelData
                                text: "• " + dialog.tilde(modelData).replace(dialog.plan.home || "\u0000", "~")
                                opacity: 0.75
                                wrapMode: Text.Wrap
                                Layout.fillWidth: true
                                Layout.leftMargin: Kirigami.Units.gridUnit * 2
                            }
                        }
                        QQC2.Label {
                            visible: modelData.action === "conflict"
                            text: qsTr("Needs a decision: %1. If selected, it is imported as “%2”.").arg(modelData.reason).arg(modelData.altId)
                            color: Kirigami.Theme.neutralTextColor
                            wrapMode: Text.Wrap
                            Layout.fillWidth: true
                            Layout.leftMargin: Kirigami.Units.gridUnit * 2
                        }
                        QQC2.Label {
                            visible: modelData.action === "skip"
                            text: qsTr("Cannot be imported: %1").arg(modelData.reason)
                            color: Kirigami.Theme.negativeTextColor
                            wrapMode: Text.Wrap
                            Layout.fillWidth: true
                            Layout.leftMargin: Kirigami.Units.gridUnit * 2
                        }
                        }
                    }
                }
            }

            QQC2.Label {
                visible: dialog.plan.skipped.length > 0
                text: dialog.plural(dialog.plan.skipped.length, qsTr("%1 other file was found but is not eligible (not an AppImage, or already imported)."), qsTr("%1 other files were found but are not eligible (not AppImages, or already imported)."))
                opacity: 0.6
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }

            RowLayout {
                Layout.alignment: Qt.AlignRight
                QQC2.Button { text: dialog.plan.items.length === 0 ? qsTr("Close") : qsTr("Cancel"); onClicked: dialog.close() }
                PrimaryButton {
                    visible: dialog.plan.items.length > 0
                    text: dialog.plural(dialog.chosen.length, qsTr("Review %1 import…"), qsTr("Review %1 imports…"))
                    icon.name: "dialog-ok"
                    enabled: dialog.chosen.length > 0
                    onClicked: dialog.phase = "confirm"
                }
            }
        }

        // ---- 3. final confirmation
        ColumnLayout {
            visible: dialog.phase === "confirm"
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: Kirigami.Units.largeSpacing
            Kirigami.Heading { text: dialog.plural(dialog.chosen.length, qsTr("Import %1 app?"), qsTr("Import %1 apps?")); level: 2 }
            Repeater {
                model: [
                    {n: dialog.staysCount, text: qsTr("%1 file(s) stay where they are.")},
                    {n: dialog.movedCount, text: qsTr("%1 file(s) are moved into the library folder.")},
                    {n: dialog.copiedCount, text: qsTr("%1 file(s) are copied into the library folder.")},
                    {n: dialog.replacesCount, text: qsTr("%1 menu entr(ies) from other tools are backed up (restorable).")},
                    {n: dialog.twiceCount, text: qsTr("%1 app(s) will appear twice in your menu (the other tool's entry stays).")}
                ]
                QQC2.Label {
                    required property var modelData
                    visible: modelData.n > 0
                    text: "• " + modelData.text.arg(modelData.n)
                    wrapMode: Text.Wrap
                    Layout.fillWidth: true
                }
            }
            QQC2.Label {
                text: qsTr("Files you did not choose, and the apps' own settings and data, are not touched. Removing an imported app later keeps its file unless you ask otherwise.")
                wrapMode: Text.Wrap
                opacity: 0.75
                Layout.fillWidth: true
            }
            Item { Layout.fillHeight: true }
            RowLayout {
                Layout.alignment: Qt.AlignRight
                QQC2.Button { text: qsTr("Back"); onClicked: dialog.phase = "plan" }
                PrimaryButton {
                    text: qsTr("Yes, import")
                    icon.name: "document-import"
                    onClicked: dialog.run()
                }
            }
        }

        // ---- 4. result
        ColumnLayout {
            visible: dialog.phase === "done"
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: Kirigami.Units.smallSpacing
            Kirigami.Heading {
                level: 2
                text: qsTr("Imported %1 of %2").arg(dialog.results.filter(function (r) { return r.ok }).length).arg(dialog.results.length)
            }
            QQC2.ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                ListView {
                    model: dialog.results
                    delegate: QQC2.Label {
                        required property var modelData
                        width: ListView.view ? ListView.view.width - Kirigami.Units.gridUnit : 0
                        wrapMode: Text.Wrap
                        text: modelData.ok ? "✔ " + modelData.id + " — " + modelData.notes.join("; ")
                                           : "✖ " + dialog.tilde(modelData.path) + " — " + modelData.error
                        color: modelData.ok ? Kirigami.Theme.textColor : Kirigami.Theme.negativeTextColor
                    }
                }
            }
            RowLayout {
                Layout.alignment: Qt.AlignRight
                QQC2.Button {
                    visible: dialog.replacedAny
                    text: qsTr("Restore the other menu entries")
                    icon.name: "edit-undo"
                    onClicked: {
                        var r = Util.parse(dialog.backend.importRestore())
                        applicationWindow().showPassiveNotification(r.error ? r.error : qsTr("Restored %1 menu entr(ies)").arg(r.restored))
                    }
                }
                PrimaryButton { text: qsTr("Done"); icon.name: "dialog-ok"; onClicked: dialog.close() }
            }
        }
    }
}
