// appOFholding's quick-settings tile for uQ (docs/integrating.md in the uQ repository): "N
// updates" for the AppImage library, from `appofholding list --json` (the last known status, no
// network). A click opens the AppImages page in uQ Settings. All text goes through ctx.UqText.
import QtQuick
import QtQuick.Layouts

Item {
    id: widget

    property var ctx
    // `appofholding list --json`, or null before the first answer.
    property var library: null
    property string problem: ""

    readonly property int updates: library && typeof library.updates === "number" ? library.updates : 0
    readonly property int appCount: library && Array.isArray(library.apps) ? library.apps.length : 0

    function line() {
        if (library === null)
            return problem !== "" ? "AppImages · unavailable" : "AppImages";
        if (appCount === 0)
            return "AppImages · none yet";
        if (updates === 0)
            return "AppImages · up to date";
        return "AppImages · " + updates + (updates === 1 ? " update" : " updates");
    }

    function refresh() {
        if (!ctx)
            return;
        ctx.command.run("list", [], function (r) {
            if (r.error === undefined && r.output && r.output.version === 1) {
                widget.library = r.output;
                widget.problem = "";
            } else {
                widget.library = null;
                widget.problem = String(r.error || "appofholding gave no list");
            }
        });
    }

    function openPage() {
        if (!ctx)
            return;
        ctx.command.run("open-page", [], function (r) {
            if (r.error !== undefined)
                widget.problem = String(r.error);
        });
    }

    Component.onCompleted: refresh()

    // Updates are found by appOFholding's timer or its window; this only reads the result.
    Timer {
        interval: 60000
        running: widget.ctx !== undefined && widget.ctx !== null
        repeat: true
        onTriggered: widget.refresh()
    }

    Rectangle {
        anchors.fill: parent
        radius: widget.ctx ? widget.ctx.Theme.radius - 2 : 4
        color: widget.ctx ? (mouse.containsMouse ? widget.ctx.Theme.overlay : widget.ctx.Theme.surfaceAlt)
                          : "transparent"
    }
    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 10
        anchors.rightMargin: 10
        spacing: 8
        Loader {
            Layout.fillWidth: true
            sourceComponent: widget.ctx ? widget.ctx.UqText : null
            onLoaded: item.text = Qt.binding(() => widget.line())
        }
        // A dot in the accent colour while updates are waiting.
        Rectangle {
            visible: widget.updates > 0
            implicitWidth: 8
            implicitHeight: 8
            radius: 4
            color: widget.ctx ? widget.ctx.Theme.accent : "blue"
        }
    }
    MouseArea {
        id: mouse
        anchors.fill: parent
        hoverEnabled: true
        onClicked: widget.openPage()
        Accessible.role: Accessible.Button
        Accessible.name: widget.line()
        Accessible.onPressAction: widget.openPage()
    }
}
