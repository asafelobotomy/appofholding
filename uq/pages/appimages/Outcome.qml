// What an action said, under its row: "Working…" while it runs, then appOFholding's own answer in
// plain words. A failure is shown in the error colour.
import QtQuick
import QtQuick.Layouts
import org.uq.settings

ColumnLayout {
    id: out

    /// The page (Page.qml): it holds the notes and which write is running.
    property var host
    /// The row's key in the page's notes.
    property string noteKey
    readonly property var note: out.host ? out.host.noteFor(out.noteKey) : null
    readonly property bool working: !!out.host && out.host.workingKey === out.noteKey

    visible: out.working || out.note !== null
    spacing: 2

    UqLabel {
        id: message
        text: out.working ? "Working…" : (out.note ? out.note.message : "")
        visible: text !== ""
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    Binding {
        target: message
        property: "color"
        value: out.host ? out.host.errorColor() : "red"
        when: !out.working && out.note !== null && out.note.failed
    }
}
