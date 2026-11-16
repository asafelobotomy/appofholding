// Loads the uQ Settings page (uq/pages/appimages) with a fake ctx, as uQ Settings would, and checks
// what it shows and the commands it runs. Run with:
//   QT_QPA_PLATFORM=offscreen /usr/lib/qt6/bin/qmltestrunner -import tests/uq/stub -input tests/uq/tst_page.qml
import QtQuick
import QtQuick.Controls as QQC2
import QtTest

Item {
    id: root
    width: 900
    height: 700

    readonly property var library: ({
            version: 1,
            updates: 1,
            apps: [
                { id: "krita", name: "Krita", version: "5.2.0", icon: "appofholding-krita", size_bytes: 250000000, has_source: true, policy: "notify", status: "update-available", latest: "5.2.3", skipped: null, error: null, update: true, scan: { state: "clean", detail: "", at: 1791386830 } },
                { id: "tool", name: "Tool", version: "1.0", icon: null, size_bytes: 1200000, has_source: true, policy: "pinned", status: "up-to-date", latest: "1.0", skipped: null, error: null, update: false, scan: { state: "threat", detail: "Win.Test.EICAR_HDB-1", at: 1791386830 } },
                { id: "broken", name: "Broken", version: "2.0", icon: null, size_bytes: 5000, has_source: true, policy: "auto", status: null, latest: null, skipped: null, error: "HTTP 404", update: false },
                { id: "local", name: "Local", version: null, icon: null, size_bytes: 100, has_source: false, policy: null, status: null, latest: null, skipped: null, error: null, update: false }
            ]
        })
    readonly property var config: ({
            version: 1,
            ok: true,
            settings: { policy: "notify", keep_versions: 2, sandbox: "none", schedule: "daily", scan: "off" },
            limits: { max_keep_versions: 10 },
            apps_with_source: 3
        })

    readonly property var base: ({
            list: { code: 0, output: root.library },
            config: { code: 0, output: root.config },
            check: { code: 0, output: null },
            "update-all": { code: 0, output: null },
            update: { code: 0, output: null },
            "scan-all": { code: 0, output: null },
            scan: { code: 0, output: null },
            policy: { code: 0, output: "krita: update policy is now auto" },
            skip: { code: 0, output: "krita: skipping 5.2.3" },
            unskip: { code: 0, output: "krita: no longer skipping any version" },
            remove: { code: 0, output: { version: 1, ok: true, id: "tool", name: "Tool", kept_file: null, kept_data: true, purged_data: false } },
            "remove-data": { code: 0, output: { version: 1, ok: true, id: "tool", name: "Tool", kept_file: null, kept_data: false, purged_data: true } },
            "config-set": { code: 0, output: { version: 1, ok: true, key: "keep_versions", settings: root.config.settings, changed_apps: 0 } },
            "config-policy-all": { code: 0, output: { version: 1, ok: true, key: "policy", settings: root.config.settings, changed_apps: 3 } }
        })

    FakeContext {
        id: fake
        commands: root.base
        events: ({
                check: [
                    { event: "start", total: 2 },
                    { event: "checked", index: 1, total: 2, id: "krita", update: true, latest: "5.2.3", error: null },
                    { event: "checked", index: 2, total: 2, id: "broken", update: false, latest: null, error: "HTTP 404" },
                    { event: "done", updates: 1, total: 2 }
                ],
                "scan-all": [
                    { event: "start", total: 2 },
                    { event: "scanned", index: 1, total: 2, id: "krita", name: "Krita", state: "clean", detail: "", at: 1 },
                    { event: "scanned", index: 2, total: 2, id: "tool", name: "Tool", state: "threat", detail: "Win.Test.EICAR_HDB-1", at: 1 },
                    { event: "done", threats: 1, errors: 0, total: 2 }
                ],
                scan: [
                    { event: "start", total: 1 },
                    { event: "scanned", index: 1, total: 1, id: "krita", name: "Krita", state: "clean", detail: "", at: 1 },
                    { event: "done", threats: 0, errors: 0, total: 1 }
                ],
                "update-all": [
                    { event: "start", app: "krita", name: "Krita", index: 1, total: 1 },
                    { event: "progress", app: "krita", percent: 40 },
                    { event: "result", app: "krita", name: "Krita", outcome: "updated", from: "5.2.0", to: "5.2.3" },
                    { event: "done", updated: 1, failed: 0, total: 1 }
                ]
            })
        // Commands that have `confirm` in page.toml.
        confirmed: ["update-all", "remove", "remove-data", "config-policy-all"]
    }

    Loader {
        id: loader
        anchors.fill: parent
        source: Qt.resolvedUrl("../../uq/pages/appimages/Page.qml")
    }

    function shown(item, out) {
        out = out || [];
        if (typeof item.text === "string" && item.visible && item.text !== "")
            out.push(item.text);
        for (let i = 0; i < item.children.length; i++)
            shown(item.children[i], out);
        if (item.contentItem && item.contentItem !== item)
            shown(item.contentItem, out);
        return out;
    }

    function find(item, pred) {
        if (pred(item))
            return item;
        const kids = item.children;
        for (let i = 0; i < kids.length; i++) {
            const f = find(kids[i], pred);
            if (f)
                return f;
        }
        if (item.contentItem && item.contentItem !== item)
            return find(item.contentItem, pred);
        return null;
    }

    function button(text) {
        return find(loader.item, i => i.text === text && i.visible && typeof i.clicked === "function" || (i.text === text && i.visible && i.clicked !== undefined));
    }

    TestCase {
        name: "AppimagesPage"
        when: windowShown

        function openTab(name) {
            const b = root.find(loader.item, i => i.objectName === "tab-" + name);
            verify(b !== null, "tab " + name);
            b.clicked();
        }

        function init() {
            fake.commands = root.base;
            fake.calls = [];
            fake.confirms = true;
            loader.source = "";
            loader.source = Qt.resolvedUrl("../../uq/pages/appimages/Page.qml");
            verify(loader.status === Loader.Ready, "the page loads");
            loader.item.ctx = fake.ctx;
        }

        function hasText(t) {
            tryVerify(() => root.shown(loader.item).indexOf(t) >= 0, 2000, t + " in " + JSON.stringify(root.shown(loader.item)));
        }

        function ran(id) {
            return fake.calls.filter(c => (c[0] === "run" || c[0] === "stream") && c[1] === id);
        }

        function test_reads_and_follows_the_library() {
            hasText("1 update for 4 AppImages");
            compare(fake.calls[0], ["watch", "list"]);
            const c = root.find(loader.item, i => i.hasOwnProperty("pageId"));
            verify(c !== null);
        }

        function test_overview_lists_updates_and_problems_in_the_windows_words() {
            hasText("Updates available");
            hasText("Needs attention");
            hasText("5.2.0 · Update available: 5.2.3");
            hasText("2.0 · Couldn't check for updates");
        }

        function test_check_now_streams_and_shows_what_it_found() {
            hasText("1 update for 4 AppImages");
            const b = root.button("Check for updates");
            verify(b !== null);
            tryVerify(() => b.enabled);
            b.clicked();
            hasText("1 update found.");
            hasText("krita: 5.2.3 is available");
            hasText("broken: HTTP 404");
            compare(ran("check")[0], ["stream", "check", []]);
        }

        function test_update_all_asks_then_streams_progress() {
            hasText("1 update for 4 AppImages");
            const b = root.button("Update all");
            tryVerify(() => b.enabled);
            b.clicked();
            hasText("1 AppImage updated.");
            hasText("Krita: updated from 5.2.0 to 5.2.3");
            compare(ran("update-all")[0], ["stream", "update-all", []]);
        }

        function test_a_declined_confirmation_changes_nothing() {
            fake.confirms = false;
            hasText("1 update for 4 AppImages");
            const b = root.button("Update all");
            tryVerify(() => b.enabled);
            b.clicked();
            wait(100);
            verify(root.shown(loader.item).indexOf("1 AppImage updated.") < 0);
            verify(root.shown(loader.item).indexOf("Working…") < 0);
            // and the buttons are usable again
            tryVerify(() => b.enabled);
        }

        function test_an_update_of_one_app() {
            hasText("1 update for 4 AppImages");
            const b = root.find(loader.item, i => i.text === "Update" && i.visible);
            verify(b !== null);
            b.clicked();
            tryVerify(() => ran("update").length === 1);
            compare(ran("update")[0], ["stream", "update", ["krita"]]);
        }

        function test_apps_tab_shows_every_app_with_policy_and_size() {
            openTab("Apps");
            hasText("4 AppImages");
            hasText("Krita");
            hasText("Local");
            hasText("1.0 · Never changed (pinned) · 1.2 MB");
            hasText("unknown version \u00b7 Updates are not set up \u00b7 100 B");
        }

        function test_preferences_read_config_and_write_one_key() {
            openTab("Preferences");
            tryVerify(() => root.find(loader.item, i => i.hasOwnProperty("value") && i.hasOwnProperty("from") && i.hasOwnProperty("editable")) !== null);
            const spin = root.find(loader.item, i => i.hasOwnProperty("value") && i.hasOwnProperty("from") && i.hasOwnProperty("editable"));
            compare(spin.value, 2);
            compare(spin.to, 10);
            spin.value = 4;
            spin.valueModified();
            tryVerify(() => ran("config-set").length === 1, 3000);
            compare(ran("config-set")[0], ["run", "config-set", ["keep_versions", "4"]]);
        }

        function test_a_refusal_is_shown_under_its_row() {
            openTab("Preferences");
            fake.commands = Object.assign({}, fake.commands, { "config-set": { code: 1, error: "exit 1", output: { version: 1, ok: false, error: "at most 10 previous versions can be kept" } } });
            tryVerify(() => root.find(loader.item, i => i.hasOwnProperty("value") && i.hasOwnProperty("from") && i.hasOwnProperty("editable")) !== null);
            const spin = root.find(loader.item, i => i.hasOwnProperty("value") && i.hasOwnProperty("from") && i.hasOwnProperty("editable"));
            spin.value = 9;
            spin.valueModified();
            hasText("at most 10 previous versions can be kept");
        }

        function test_a_threat_is_marked_on_its_row_and_the_overview_warns() {
            hasText("oysterAV found a threat in 1 AppImage");
            openTab("Apps");
            hasText("oysterAV found Win.Test.EICAR_HDB-1. Don't run it; oysterAV can move it away.");
            hasText("oysterAV found no threats.");
        }

        function test_scanning_everything_streams_and_names_what_it_found() {
            hasText("1 update for 4 AppImages");
            const b = root.find(loader.item, i => i.objectName === "scan-all");
            verify(b !== null && b.enabled);
            b.clicked();
            tryVerify(() => ran("scan-all").length === 1);
            hasText("1 threat found.");
            hasText("Tool: oysterAV found Win.Test.EICAR_HDB-1");
        }

        function test_scanning_one_app_sends_only_its_id() {
            hasText("1 update for 4 AppImages");
            loader.item.stream("scan", "scan", ["krita"], "app:krita");
            tryVerify(() => ran("scan").length === 1);
            compare(ran("scan")[0], ["stream", "scan", ["krita"]]);
            loader.item.stream("scan", "scan", ["--all"], "app:x");
            wait(50);
            compare(ran("scan").length, 1);
        }

        function test_the_scan_mode_is_chosen_in_preferences() {
            openTab("Preferences");
            tryVerify(() => root.find(loader.item, i => i.objectName === "scan-mode") !== null);
            const box = root.find(loader.item, i => i.objectName === "scan-mode");
            compare(box.currentIndex, 0);
            box.currentIndex = 2;
            box.activated(2);
            tryVerify(() => ran("config-set").length === 1);
            compare(ran("config-set")[0], ["run", "config-set", ["scan", "block"]]);
            hasText("AppImages are not scanned unless you ask.");
        }

        function test_remove_runs_the_confirmed_command_and_says_what_stayed() {
            hasText("1 update for 4 AppImages");
            loader.item.write("app:tool", "remove", ["tool"], null, "section");
            tryVerify(() => ran("remove").length === 1);
            compare(ran("remove")[0], ["run", "remove", ["tool"]]);
            openTab("Apps");
            hasText("Removed Tool. Its data was kept.");
        }

        function test_remove_with_data_says_it_was_deleted() {
            hasText("1 update for 4 AppImages");
            loader.item.write("app:tool", "remove-data", ["tool"], null, "section");
            tryVerify(() => ran("remove-data").length === 1);
            openTab("Apps");
            hasText("Removed Tool. Its data was deleted.");
        }

        function test_a_declined_removal_says_nothing() {
            fake.confirms = false;
            hasText("1 update for 4 AppImages");
            loader.item.write("app:tool", "remove", ["tool"], null, "section");
            wait(100);
            openTab("Apps");
            verify(root.shown(loader.item).filter(t => t.indexOf("Removed") === 0).length === 0);
            compare(loader.item.workingKey, "");
        }

        function test_an_argument_uq_would_refuse_is_never_sent() {
            hasText("1 update for 4 AppImages");
            loader.item.write("app:x", "remove", ["--purge-data"]);
            loader.item.write("app:x", "policy", ["krita", "sometimes"]);
            wait(50);
            compare(ran("remove").length, 0);
            compare(ran("policy").length, 0);
            compare(loader.item.noteFor("app:x").failed, true);
        }

        function test_a_policy_change_and_skip_run_their_commands() {
            hasText("1 update for 4 AppImages");
            loader.item.write("app:krita", "policy", ["krita", "auto"]);
            tryVerify(() => ran("policy").length === 1);
            compare(ran("policy")[0], ["run", "policy", ["krita", "auto"]]);
            hasText("krita: update policy is now auto");
            tryVerify(() => loader.item.workingKey === "");
            loader.item.write("app:krita", "skip", ["krita"]);
            tryVerify(() => ran("skip").length === 1);
        }

        function test_apply_policy_to_all_asks_and_reports_how_many_changed() {
            openTab("Preferences");
            tryVerify(() => root.find(loader.item, i => i.text === "Apply to all apps\u2026" && i.enabled) !== null);
            root.find(loader.item, i => i.text === "Apply to all apps\u2026").clicked();
            tryVerify(() => ran("config-policy-all").length === 1);
            compare(ran("config-policy-all")[0], ["run", "config-policy-all", ["notify"]]);
            hasText("Changed 3 apps.");
        }

        function test_unreadable_library_says_so() {
            fake.commands = Object.assign({}, fake.commands, { list: { error: "appofholding: not found" } });
            loader.source = "";
            loader.source = Qt.resolvedUrl("../../uq/pages/appimages/Page.qml");
            loader.item.ctx = fake.ctx;
            hasText("AppImages are unavailable");
            hasText("appofholding: not found");
        }
    }
}
