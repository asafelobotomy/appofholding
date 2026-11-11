// appOFholding's AppImages page in uQ Settings (docs/settings-pages.md in the uQ repository):
// the library with each app's update status and policy, "Check now", "Update all", and the
// background check schedule. Everything runs through the commands declared in page.toml; all
// text goes through UqLabel (plain text only).
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings

Item {
    id: page

    property var ctx
    /// The [[setting]] id Settings search jumped to.
    property string highlight

    // `appofholding list --json` and `schedule status --json`, or null before they answer.
    property var library: null
    property var schedule: null
    // The command that is running ("" when none), so buttons wait for it.
    property string running: ""
    // Why the library couldn't be read, and why the last action failed.
    property string loadError: ""
    property string actionError: ""
    readonly property string problem: actionError !== "" ? actionError : loadError
    // The last output of an update or schedule change, shown under the buttons.
    property string note: ""

    readonly property var apps: library && Array.isArray(library.apps) ? library.apps : []
    readonly property int updates: library && typeof library.updates === "number" ? library.updates : 0

    readonly property var policyKeys: ["auto", "notify", "manual", "pinned"]
    readonly property var policyNames: ["Install automatically", "Tell me", "Stay quiet, I'll update myself", "Never change this app"]
    readonly property var intervalKeys: ["", "hourly", "daily", "weekly"]
    readonly property var intervalNames: ["Off", "Every hour", "Every day", "Every week"]

    function color(name, fallback) {
        const c = page.ctx && page.ctx.theme && page.ctx.theme.colors;
        return (c && c[name]) || fallback;
    }

    function errorOf(result) {
        if (result.error === undefined)
            return "";
        const out = typeof result.output === "string" ? result.output.trim() : "";
        return out !== "" ? String(result.error) + "\n" + out : String(result.error);
    }

    function load() {
        if (!page.ctx)
            return;
        page.ctx.command.run("list", [], result => {
            if (result.error !== undefined || !result.output || result.output.version !== 1) {
                page.loadError = String(result.error || "appofholding gave no list");
                return;
            }
            page.loadError = "";
            page.library = result.output;
        });
        page.ctx.command.run("schedule-status", [], result => {
            page.schedule = result.error === undefined && result.output ? result.output : null;
        });
    }

    // Run a command that changes something, show what it said, then read everything again.
    function act(id, args) {
        if (!page.ctx || page.running !== "")
            return;
        page.running = id;
        page.note = "";
        page.actionError = "";
        page.ctx.command.run(id, args, result => {
            page.running = "";
            if (result.cancelled)
                return;
            const err = page.errorOf(result);
            page.actionError = err;
            if (err === "" && typeof result.output === "string")
                page.note = result.output.trim();
            page.load();
        });
    }

    function checkNow() {
        if (!page.ctx || page.running !== "")
            return;
        page.running = "check";
        page.note = "";
        page.actionError = "";
        page.ctx.command.run("check", [], result => {
            page.running = "";
            if (result.error !== undefined) {
                page.actionError = String(result.error);
            } else {
                const n = result.output && typeof result.output.updates === "number" ? result.output.updates : 0;
                page.note = n === 0 ? "No updates found." : (n === 1 ? "1 update found." : n + " updates found.");
            }
            page.load();   // the check saved its results; the list shows them
        });
    }

    // The combo index of an installed timer's interval; "Every day" when it was edited by hand.
    function intervalIndex(interval) {
        const i = page.intervalKeys.indexOf(String(interval));
        return i > 0 ? i : 2;
    }

    function statusLine(app) {
        const version = app.version ? String(app.version) : "unknown version";
        if (!app.has_source)
            return version + " · no update source";
        if (app.policy === "pinned")
            return version + " · pinned";
        if (app.error)
            return version + " · check failed: " + String(app.error);
        if (app.update)
            return version + " → " + String(app.latest);
        if (app.status === "up-to-date")
            return version + " · up to date";
        if (app.status === "update-available" && app.skipped)
            return version + " · skipping " + String(app.skipped);
        if (app.status === "unknown")
            return version + " · can't tell (latest " + String(app.latest) + ")";
        return version + " · not checked yet";
    }

    onCtxChanged: load()

    QQC2.ScrollView {
        anchors.fill: parent
        contentWidth: availableWidth

        ColumnLayout {
            width: parent.width
            spacing: 12

            // ---- summary and the two big actions
            ColumnLayout {
                Layout.fillWidth: true
                Layout.margins: 16
                spacing: 8

                UqLabel {
                    Layout.fillWidth: true
                    font.bold: true
                    text: page.library === null ? (page.problem !== "" ? "AppImages are unavailable" : "Reading the library…")
                        : page.apps.length === 0 ? "No AppImages yet"
                        : page.updates === 0 ? page.apps.length + (page.apps.length === 1 ? " AppImage, up to date" : " AppImages, all up to date")
                        : page.updates + (page.updates === 1 ? " update" : " updates") + " for " + page.apps.length + " AppImages"
                }
                RowLayout {
                    spacing: 8
                    QQC2.Button {
                        text: "Check now"
                        icon.name: "view-refresh"
                        enabled: page.running === "" && page.library !== null
                        onClicked: page.checkNow()
                    }
                    QQC2.Button {
                        objectName: "update-all"
                        text: "Update all"
                        icon.name: "system-software-update"
                        enabled: page.running === "" && page.updates > 0
                        highlighted: page.highlight === "update-all"
                        onClicked: page.act("update-all", [])
                    }
                    QQC2.BusyIndicator {
                        visible: page.running !== ""
                        running: visible
                        implicitWidth: 24
                        implicitHeight: 24
                    }
                }
                UqLabel {
                    Layout.fillWidth: true
                    visible: page.problem !== ""
                    text: page.problem
                    color: page.color("error", "red")
                    wrapMode: Text.Wrap
                }
                UqLabel {
                    Layout.fillWidth: true
                    visible: page.note !== ""
                    text: page.note
                    color: page.color("text_muted", "gray")
                    wrapMode: Text.Wrap
                    maximumLineCount: 12
                    elide: Text.ElideRight
                }
            }

            // ---- the library
            ColumnLayout {
                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                spacing: 4

                UqLabel {
                    text: "Apps and their update policy"
                    font.bold: true
                    color: page.highlight === "policy" || page.highlight === "apps" ? page.color("accent", "steelblue")
                                                                                       : page.color("text", "black")
                }
                Repeater {
                    model: page.apps
                    delegate: RowLayout {
                        required property var modelData
                        Layout.fillWidth: true
                        spacing: 8
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 0
                            UqLabel {
                                Layout.fillWidth: true
                                text: String(modelData.name)
                                elide: Text.ElideRight
                            }
                            UqLabel {
                                Layout.fillWidth: true
                                text: page.statusLine(modelData)
                                color: modelData.update ? page.color("accent", "steelblue") : page.color("text_muted", "gray")
                                elide: Text.ElideRight
                            }
                        }
                        QQC2.Button {
                            visible: modelData.update === true
                            text: "Update"
                            enabled: page.running === ""
                            onClicked: page.act("update", [String(modelData.id)])
                        }
                        QQC2.ComboBox {
                            visible: modelData.has_source === true
                            enabled: page.running === ""
                            implicitWidth: 230
                            model: page.policyNames
                            currentIndex: Math.max(0, page.policyKeys.indexOf(String(modelData.policy)))
                            onActivated: index => page.act("policy", [String(modelData.id), page.policyKeys[index]])
                        }
                    }
                }
            }

            // ---- background checks
            ColumnLayout {
                Layout.fillWidth: true
                Layout.margins: 16
                spacing: 4

                UqLabel {
                    text: "Background update checks"
                    font.bold: true
                    color: page.highlight === "schedule" ? page.color("accent", "steelblue") : page.color("text", "black")
                }
                UqLabel {
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    color: page.color("text_muted", "gray")
                    text: "A timer checks your AppImages, installs the ones set to install automatically and tells you about the rest."
                }
                QQC2.ComboBox {
                    id: interval
                    objectName: "interval"
                    implicitWidth: 230
                    enabled: page.running === "" && page.schedule !== null
                    model: page.intervalNames
                    currentIndex: page.schedule && page.schedule.installed
                        ? page.intervalIndex(page.schedule.interval) : 0
                    onActivated: index => {
                        if (index === 0) {
                            page.act("schedule-remove", []);
                        } else {
                            page.act("schedule-install", [page.intervalKeys[index]]);
                        }
                        // Show the saved state until the commands answer.
                        currentIndex = Qt.binding(() => page.schedule && page.schedule.installed
                            ? page.intervalIndex(page.schedule.interval) : 0);
                    }
                }
            }
        }
    }
}
