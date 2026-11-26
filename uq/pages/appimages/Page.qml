// appOFholding's AppImages page in uQ Settings (docs/settings-pages.md in the uQ repository): what
// is in the library and how each app is doing, checking and updating with progress, removal
// with a preview, and the settings every app shares. Everything goes through the commands
// declared in page.toml; logic.js reads their answers and words the result. The page never
// forces anything: appOFholding's own refusals are shown as they are.
// Laid out like Settings' own pages (uq_api = 3): three tabs of cards. The window of
// appOFholding has the same things under the same names.
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings
import "logic.js" as Logic

import "progress.js" as Progress
SettingsPage {
    id: page

    property var ctx
    property var theme
    /// The [[setting]] row Settings search jumped to; the rows flash, and their tab opens.
    property string highlight
    /// The library (Logic.libraryView), null until `list` answers.
    property var library: null
    /// The key of the row whose write or run is going, "" when none. One at a time.
    property string workingKey: ""
    /// A check or update that is running or has just finished (Progress.reduce), and whose it is.
    property var progress: null
    property string progressKey: ""
    property bool streaming: false
    readonly property bool busy: page.workingKey !== "" || page.streaming
    /// What each action said, by row key: {message, failed}.
    property var notes: ({})
    property var openedWith: null

    pageId: "appimages"

    function noteFor(key) {
        return page.notes[key] || null;
    }

    function color(name, fallback) {
        const colors = page.ctx && page.ctx.theme ? page.ctx.theme.colors : null;
        return (colors && colors[name]) || fallback;
    }

    function errorColor() {
        return page.color("error", "red");
    }

    function accentColor() {
        return page.color("accent", "steelblue");
    }

    function textColor() {
        return page.color("text", "black");
    }

    function showLibrary(result) {
        const next = Logic.libraryView(result);
        if (next !== null)
            page.library = next;
    }

    /// Reads the library again (it is also followed, so this is for after an action).
    function reload() {
        if (page.ctx)
            page.ctx.command.run("list", [], page.showLibrary);
    }

    /// Runs one action (a command that changes something) for the row `key`, then `done(result)`
    /// unless the person cancelled. `sectionKey` also shows the answer under the whole card, for
    /// a row that may be gone after it.
    function write(key, command, args, done, sectionKey) {
        if (page.busy || !page.ctx)
            return;
        const why = Logic.argsProblem(command, args);
        if (why !== "") {
            page.notes = Logic.withNote(page.notes, key, { message: why, failed: true });
            return;
        }
        page.notes = Logic.withNote(page.notes, key, null);
        if (sectionKey)
            page.notes = Logic.withNote(page.notes, sectionKey, null);
        page.workingKey = key;
        page.ctx.command.run(command, args, result => {
            page.workingKey = "";
            if (Logic.isCancelled(result))
                return;
            const note = command.indexOf("remove") === 0 ? Logic.removeOutcome(result)
                : command === "rollback" ? Logic.rollbackOutcome(result)
                : command === "add" ? Logic.addOutcome(result)
                : command.indexOf("config") === 0 ? page.configNote(result)
                : Logic.textOutcome(result);
            page.notes = Logic.withNote(page.notes, key, note);
            if (sectionKey)
                page.notes = Logic.withNote(page.notes, sectionKey, note);
            if (done)
                done(result);
            page.reload();
        });
    }

    /// What `config set` said: nothing when it worked (the setting follows the file), the
    /// reason when appOFholding refused.
    function configNote(result) {
        const env = Logic.envelope(result);
        if (env.ok) {
            const n = env.output && typeof env.output.changed_apps === "number" ? env.output.changed_apps : 0;
            return n > 0 ? { message: "Changed " + Logic.plural(n, "app", "apps") + ".", failed: false } : null;
        }
        return { message: env.error, failed: true };
    }

    /// Runs a command that streams (check, update): its events drive `progress`.
    function stream(kind, command, args, key) {
        if (page.busy || !page.ctx)
            return;
        const why = Logic.argsProblem(command, args);
        if (why !== "") {
            page.notes = Logic.withNote(page.notes, key, { message: why, failed: true });
            return;
        }
        page.notes = Logic.withNote(page.notes, key, null);
        page.progress = Progress.startProgress(kind);
        page.progressKey = key;
        page.streaming = true;
        page.ctx.command.stream(command, args, event => {
            page.progress = Progress.reduce(page.progress, event);
        }, result => {
            page.streaming = false;
            if (Logic.isCancelled(result)) {
                page.progress = null;
                page.progressKey = "";
                return;
            }
            const problem = Progress.streamProblem(page.progress, result);
            if (problem !== "")
                page.notes = Logic.withNote(page.notes, key, { message: problem, failed: true });
            page.reload();
        });
    }

    // Follow the library from the first moment: the summary, the updates and every row come from it.
    onCtxChanged: {
        if (page.ctx && page.ctx !== page.openedWith) {
            page.openedWith = page.ctx;
            page.ctx.command.watch("list", page.showLibrary);
        }
    }

    SettingsTab {
        title: "Overview"
        iconName: "view-list-details"

        SettingsCard {
            title: "Updates"
            iconName: "system-software-update"
            help: "Checking asks each app's update source. An update keeps the version it replaces, so you can go back."
            wide: true
            Layout.fillWidth: true

            SettingRow {
                settingId: "updates"
                label: Logic.summary(page.library)
                highlight: page.highlight
                Layout.fillWidth: true

                QQC2.Button {
                    text: "Check for updates"
                    icon.name: "view-refresh"
                    enabled: !page.busy && page.library !== null && page.library.problem === ""
                    onClicked: page.stream("check", "check", [], "all")
                }
                QQC2.Button {
                    objectName: "scan-all"
                    text: "Scan with oysterAV"
                    icon.name: "security-medium"
                    enabled: !page.busy && page.library !== null && page.library.problem === "" && page.library.apps.length > 0
                    onClicked: page.stream("scan", "scan-all", [], "all")
                }
                QQC2.Button {
                    objectName: "update-all"
                    text: "Update all"
                    icon.name: "system-software-update"
                    enabled: !page.busy && page.library !== null && page.library.updates > 0
                    onClicked: page.stream("update", "update-all", [], "all")
                }
            }
            ErrorCard {
                objectName: "scan-threats"
                title: page.library !== null ? "oysterAV found a threat in " + Logic.plural(Logic.threatCount(page.library.apps), "AppImage", "AppImages") : ""
                detail: "Don't run them. They are marked in Your AppImages, and oysterAV can move them away."
                visible: page.library !== null && page.library.problem === "" && Logic.threatCount(page.library.apps) > 0
                Layout.fillWidth: true
            }
            ErrorCard {
                title: "Couldn't read your AppImages"
                detail: page.library ? page.library.problem : ""
                visible: page.library !== null && page.library.problem !== ""
                Layout.fillWidth: true
            }
            ProgressView {
                host: page
                progressKey: "all"
                Layout.fillWidth: true
            }
            Outcome {
                host: page
                noteKey: "all"
                Layout.fillWidth: true
            }
        }

        Repeater {
            model: page.library !== null && page.library.problem === "" ? Logic.groups(page.library.apps).filter(g => g.title !== "Apps") : []

            SettingsCard {
                id: group

                required property var modelData

                title: group.modelData.title
                iconName: group.modelData.title === "Needs attention" ? "dialog-warning" : "software-update-available"
                wide: true
                Layout.fillWidth: true

                Repeater {
                    model: group.modelData.apps

                    AppRow {
                        required property var modelData

                        host: page
                        app: modelData
                        full: false
                        Layout.fillWidth: true
                    }
                }
            }
        }
    }

    SettingsTab {
        title: "Apps"
        iconName: "applications-all"

        SettingsCard {
            title: "Your AppImages"
            iconName: "applications-all"
            help: "Remove and going back show what they would change before they change anything. Importing AppImages you already have is done in appOFholding."
            wide: true
            Layout.fillWidth: true
            actions: [
                QQC2.Button {
                    text: "Add or import…"
                    icon.name: "list-add"
                    onClicked: page.ctx.openApp("io.github.appofholding.desktop")
                }
            ]

            SettingRow {
                settingId: "apps"
                label: page.library !== null && page.library.problem === "" ? Logic.plural(page.library.apps.length, "AppImage", "AppImages") : "AppImages"
                highlight: page.highlight
                Layout.fillWidth: true
            }
            SettingRow {
                settingId: "add"
                label: "Add an AppImage from a link"
                highlight: page.highlight
                Layout.fillWidth: true

                QQC2.TextField {
                    id: link
                    objectName: "add-link"
                    placeholderText: "GitHub link or owner/repo"
                    implicitWidth: 260
                    enabled: !page.busy
                    onAccepted: addButton.clicked()
                }
                QQC2.Button {
                    id: addButton
                    objectName: "add-button"
                    text: "Add"
                    icon.name: "list-add"
                    enabled: !page.busy && link.text.trim() !== ""
                    onClicked: page.write("add", "add", [link.text.trim()], result => {
                        if (Logic.envelope(result).ok)
                            link.text = "";
                    })
                }
            }
            Outcome {
                host: page
                noteKey: "add"
                Layout.fillWidth: true
            }
            UqLabel {
                text: "Nothing here yet. Add an AppImage from a link above, or import the ones you have in appOFholding."
                visible: page.library !== null && page.library.problem === "" && page.library.apps.length === 0
                opacity: 0.7
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
            Outcome {
                host: page
                noteKey: "section"
                Layout.fillWidth: true
            }
            Repeater {
                model: page.library !== null && page.library.problem === "" ? Logic.groups(page.library.apps).reduce((all, g) => all.concat(g.apps), []) : []

                AppRow {
                    required property var modelData

                    host: page
                    app: modelData
                    Layout.fillWidth: true
                }
            }
        }
    }

    SettingsTab {
        title: "Preferences"
        iconName: "configure"

        PreferencesSection {
            host: page
            Layout.fillWidth: true
        }
    }
}
