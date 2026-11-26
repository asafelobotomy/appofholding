// A check or an update that is running, or has just finished: a bar that follows the events
// (Progress.reduce), the line saying what is happening, and what each app answered.
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings
import "progress.js" as Progress
ColumnLayout {
    id: view

    property var host
    /// The progress key this view shows ("all", or "app:<id>"): it is empty for other runs.
    property string progressKey
    readonly property var state: view.host && view.host.progressKey === view.progressKey ? view.host.progress : null

    visible: view.state !== null
    spacing: 4

    QQC2.ProgressBar {
        visible: view.state !== null && !view.state.done
        indeterminate: view.state !== null && Progress.fraction(view.state) < 0
        value: view.state !== null ? Math.max(0, Progress.fraction(view.state)) : 0
        Layout.fillWidth: true
    }
    UqLabel {
        text: view.state !== null ? Progress.progressLine(view.state) : ""
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    Repeater {
        model: view.state !== null ? view.state.results : []

        UqLabel {
            id: line

            required property var modelData

            text: line.modelData.text
            wrapMode: Text.Wrap
            color: line.modelData.outcome === "failed" && view.host ? view.host.errorColor() : (view.host ? view.host.textColor() : "black")
            opacity: line.modelData.outcome === "failed" ? 1 : 0.8
            Layout.fillWidth: true
        }
    }
}
