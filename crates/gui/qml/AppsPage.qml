import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

ThemedPage {
    id: page
    title: qsTr("My apps")

    property string query: ""
    readonly property var allApps: applicationWindow().apps
    readonly property int updateCount: allApps.filter(function (a) { return a.status === "update-available" }).length

    // Most urgent first: updates, then apps whose check failed, then everything else by name.
    function rank(a) { return a.status === "update-available" ? 0 : (a.error ? 1 : 2) }
    function groupName(a) {
        var r = rank(a)
        return r === 0 ? qsTr("Updates available") : r === 1 ? qsTr("Needs attention") : qsTr("Apps")
    }
    readonly property var rows: {
        var q = query.trim().toLowerCase()
        var out = allApps.filter(function (a) { return !q || a.name.toLowerCase().indexOf(q) >= 0 || a.id.indexOf(q) >= 0 })
        out = out.map(function (a) { return Object.assign({group: groupName(a)}, a) })
        out.sort(function (a, b) { return rank(a) - rank(b) || a.name.toLowerCase().localeCompare(b.name.toLowerCase()) })
        return out
    }

    // How the apps are laid out: "list", "grid" or "compact". Saved, and applied at once.
    property string viewMode: applicationWindow().backendObject.view()
    function setViewMode(mode) {
        viewMode = mode
        var err = applicationWindow().backendObject.setView(mode)
        if (err) applicationWindow().showPassiveNotification(err)
    }

    // The grid is a list of lines, each holding as many tiles as fit, so section headers,
    // scrolling and recycling all keep working.
    readonly property real gridMin: Kirigami.Units.gridUnit * 12
    readonly property int columns: Math.max(1, Math.floor((list.width - list.leftMargin - list.rightMargin + list.spacing) / (gridMin + list.spacing)))
    readonly property var lines: {
        var out = []
        var n = columns
        for (var i = 0; i < rows.length; i++) {
            var last = out.length ? out[out.length - 1] : null
            if (last && last.group === rows[i].group && last.items.length < n) last.items.push(rows[i])
            else out.push({group: rows[i].group, items: [rows[i]]})
        }
        return out
    }

    actions: [
        Kirigami.Action {
            text: qsTr("Add app")
            icon.name: "list-add"
            Kirigami.Action {
                text: qsTr("Import existing AppImages…")
                icon.name: "document-import"
                onTriggered: applicationWindow().openImportDialog()
            }
            Kirigami.Action {
                text: qsTr("From a link…")
                icon.name: "download"
                onTriggered: applicationWindow().openAddDialog()
            }
            Kirigami.Action {
                text: qsTr("From a file…")
                icon.name: "document-open"
                onTriggered: applicationWindow().openFileDialog()
            }
        },
        Kirigami.Action {
            text: qsTr("Check for updates")
            icon.name: "view-refresh"
            enabled: !applicationWindow().working
            onTriggered: applicationWindow().checkAll()
        }
    ]

    header: ColumnLayout {
        spacing: Kirigami.Units.smallSpacing
        visible: allApps.length > 0 || !!applicationWindow().selfInfo

        Kirigami.InlineMessage {
            id: selfMessage
            readonly property var info: applicationWindow().selfInfo
            Layout.fillWidth: true
            Layout.margins: Kirigami.Units.smallSpacing
            visible: !!info && info.available
            type: Kirigami.MessageType.Information
            text: info ? qsTr("appOFholding %1 is available (you have %2).").arg(info.latest).arg(info.current) : ""
            actions: [
                Kirigami.Action {
                    text: qsTr("What's new")
                    icon.name: "help-about"
                    onTriggered: Qt.openUrlExternally(selfMessage.info.url)
                },
                Kirigami.Action {
                    readonly property bool busy: applicationWindow().progress["@selfUpdate"] !== undefined
                    text: selfMessage.info && selfMessage.info.canInstall
                          ? (busy ? qsTr("Updating…") : qsTr("Update now"))
                          : qsTr("How to update")
                    icon.name: "update-none"
                    enabled: !busy
                    onTriggered: {
                        var i = selfMessage.info
                        if (i.canInstall) applicationWindow().confirmSelfUpdate()
                        else if (i.command) applicationWindow().showPassiveNotification(qsTr("Run in a terminal: %1").arg(i.command))
                        else Qt.openUrlExternally(i.url)
                    }
                }
            ]
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            Layout.margins: Kirigami.Units.smallSpacing
            visible: page.updateCount > 0
            type: Kirigami.MessageType.Positive
            text: page.updateCount === 1 ? qsTr("1 update available") : qsTr("%1 updates available").arg(page.updateCount)
            actions: [
                Kirigami.Action {
                    text: qsTr("Update all")
                    icon.name: "software-update-available"
                    onTriggered: {
                        var a = page.allApps
                        var busy = applicationWindow().progress
                        for (var i = 0; i < a.length; i++)
                            if (a[i].status === "update-available" && busy[a[i].id] === undefined)
                                applicationWindow().backendObject.start("update", a[i].id, "", "", "")
                    }
                }
            ]
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.margins: Kirigami.Units.smallSpacing
            visible: page.allApps.length > 0
            spacing: Kirigami.Units.smallSpacing

            Kirigami.SearchField {
                id: search
                visible: page.allApps.length > 4
                placeholderText: qsTr("Search apps")
                Layout.fillWidth: true
                onTextChanged: page.query = text
            }
            Item { Layout.fillWidth: true; visible: !search.visible }

            QQC2.ButtonGroup { id: viewGroup }
            Repeater {
                model: [
                    {mode: "grid", icon: "view-list-icons", label: qsTr("Grid")},
                    {mode: "list", icon: "view-list-details", label: qsTr("List")},
                    {mode: "compact", icon: "view-list-text", label: qsTr("List (Compact)")}
                ]
                QQC2.ToolButton {
                    required property var modelData
                    icon.name: modelData.icon
                    checkable: true
                    checked: page.viewMode === modelData.mode
                    QQC2.ButtonGroup.group: viewGroup
                    Accessible.name: modelData.label
                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.text: modelData.label
                    onClicked: page.setViewMode(modelData.mode)
                }
            }
        }
    }

    ListView {
        id: list
        model: page.viewMode === "grid" ? page.lines : page.rows
        spacing: page.viewMode === "compact" ? 2 : Kirigami.Units.smallSpacing
        topMargin: Kirigami.Units.smallSpacing
        bottomMargin: Kirigami.Units.largeSpacing
        leftMargin: Kirigami.Units.smallSpacing
        rightMargin: Kirigami.Units.smallSpacing
        clip: true
        reuseItems: false

        section.property: "group"
        section.criteria: ViewSection.FullString
        section.delegate: Kirigami.ListSectionHeader {
            width: ListView.view ? ListView.view.width : implicitWidth
            label: section
        }
        delegate: page.viewMode === "grid" ? gridLine : page.viewMode === "compact" ? compactRow : listCard

        Component { id: listCard; AppCard {} }
        Component { id: compactRow; AppRow {} }
        Component {
            id: gridLine
            Item {
                id: line
                required property var modelData
                readonly property real cell: (width - (page.columns - 1) * list.spacing) / page.columns
                width: list.width - list.leftMargin - list.rightMargin
                implicitHeight: tiles.height
                height: tiles.height
                Row {
                    id: tiles
                    spacing: list.spacing
                    Repeater {
                        model: line.modelData.items
                        AppGridCard {
                            required property var modelData
                            app: modelData
                            width: line.cell
                            // Tiles in a line share the tallest height so the edges align.
                            height: Math.max(implicitHeight, Kirigami.Units.gridUnit * 10)
                        }
                    }
                }
            }
        }

        add: Transition { NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Kirigami.Units.longDuration } }
        displaced: Transition { NumberAnimation { properties: "y"; duration: Kirigami.Units.longDuration; easing.type: Easing.OutCubic } }

        Kirigami.PlaceholderMessage {
            anchors.centerIn: parent
            width: parent.width - Kirigami.Units.gridUnit * 4
            visible: list.count === 0
            icon.source: "qrc:/appofholding/app-icon.svg"
            text: page.allApps.length === 0 ? qsTr("Your AppImages will live here") : qsTr("No app matches “%1”").arg(page.query)
            explanation: page.allApps.length === 0
                ? qsTr("Bring in the AppImages you already have, or add one from a GitHub link. Drop a file here to add it.")
                : ""
            helpfulAction: page.allApps.length === 0 ? importAction : null
            Kirigami.Action {
                id: importAction
                text: qsTr("Import existing AppImages…")
                icon.name: "document-import"
                onTriggered: applicationWindow().openImportDialog()
            }
        }
    }

    DropArea {
        anchors.fill: parent
        onDropped: (drop) => {
            for (var i = 0; i < drop.urls.length; i++) {
                applicationWindow().backendObject.start("addFile", "", drop.urls[i].toString(), "", "")
            }
        }
        Rectangle {
            anchors.fill: parent
            visible: parent.containsDrag
            color: Qt.rgba(Kirigami.Theme.highlightColor.r, Kirigami.Theme.highlightColor.g, Kirigami.Theme.highlightColor.b, 0.15)
            border.color: Kirigami.Theme.highlightColor
            border.width: 2
            radius: Kirigami.Units.cornerRadius
            Kirigami.Heading {
                anchors.centerIn: parent
                level: 2
                text: qsTr("Drop to add")
            }
        }
    }
}
