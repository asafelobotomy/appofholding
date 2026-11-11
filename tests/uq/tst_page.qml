// Loads the uQ Settings page (uq/pages/appimages) with a fake ctx, as uQ Settings would, and
// checks what it shows and the commands it runs. Run with:
//   QT_QPA_PLATFORM=offscreen qmltestrunner -import tests/uq/imports -input tests/uq/tst_page.qml
import QtQuick
import QtQuick.Controls as QQC2
import QtTest

Item {
    id: root
    width: 700; height: 600

    property var calls: []
    property var answers: ({})
    property var ctx: ({
        theme: { colors: { text: "black", text_muted: "gray", accent: "blue", error: "red" } },
        command: { run: function (id, args, cb) {
            root.calls.push({ id: id, args: args });
            var a = root.answers[id] || { code: 0, output: null };
            Qt.callLater(function () { cb(a); });
        } }
    })

    readonly property var lib: ({ version: 1, updates: 1, apps: [
        { id: "krita", name: "Krita", version: "5.2.0", has_source: true, policy: "notify",
          status: "update-available", latest: "5.2.3", update: true, error: null, skipped: null },
        { id: "tool", name: "Tool", version: "1.0", has_source: true, policy: "pinned",
          status: "up-to-date", latest: "1.0", update: false, error: null, skipped: null },
        { id: "local", name: "Local", version: null, has_source: false, policy: null,
          status: null, latest: null, update: false, error: null, skipped: null } ] })

    Loader {
        id: loader
        anchors.fill: parent
        source: Qt.resolvedUrl("../../uq/pages/appimages/Page.qml")
    }

    TestCase {
        name: "AppimagesPage"
        when: windowShown

        function shown(item, out) {
            out = out || [];
            if (typeof item.text === "string" && item.visible && item.text !== "") out.push(item.text);
            for (var i = 0; i < item.children.length; i++) shown(item.children[i], out);
            if (item.contentItem && item.contentItem !== item) shown(item.contentItem, out);
            return out;
        }
        function find(item, pred) {
            if (pred(item)) return item;
            var kids = item.children;
            for (var i = 0; i < kids.length; i++) { var f = find(kids[i], pred); if (f) return f; }
            if (item.contentItem && item.contentItem !== item) return find(item.contentItem, pred);
            return null;
        }
        function hasText(t) {
            tryVerify(function () { return shown(loader.item).indexOf(t) >= 0; }, 2000,
                      t + " in " + JSON.stringify(shown(loader.item)));
        }

        function init() {
            root.calls = [];
            root.answers = { list: { code: 0, output: root.lib },
                             "schedule-status": { code: 0, output: { version: 1, installed: true, interval: "weekly" } } };
            loader.item.ctx = null;
            loader.item.ctx = root.ctx;
        }

        function test_shows_the_library_and_schedule() {
            hasText("1 update for 3 AppImages");
            hasText("5.2.0 → 5.2.3");
            hasText("1.0 · pinned");
            hasText("unknown version · no update source");
            var combo = find(loader.item, function (i) { return i.objectName === "interval"; });
            verify(combo);
            tryCompare(combo, "currentIndex", 3);
        }

        function test_update_all_and_policy() {
            hasText("1 update for 3 AppImages");
            root.answers["update-all"] = { code: 0, output: "krita: updated 5.2.0 -> 5.2.3\n" };
            var b = find(loader.item, function (i) { return i.objectName === "update-all"; });
            verify(b && b.enabled);
            mouseClick(b);
            hasText("krita: updated 5.2.0 -> 5.2.3");
            compare(root.calls.filter(function (c) { return c.id === "update-all"; }).length, 1);

            root.calls = [];
            loader.item.act("policy", ["krita", "auto"]);
            tryVerify(function () { return root.calls.length >= 2; });
            compare(root.calls[0].args, ["krita", "auto"]);
            compare(root.calls[1].id, "list");
        }

        function test_errors_are_shown() {
            root.answers.update = { code: 1, error: "exit status 1", output: "krita: error: no network\n" };
            loader.item.act("update", ["krita"]);
            hasText("exit status 1\nkrita: error: no network");
            root.answers.check = { error: "timed out" };
            loader.item.checkNow();
            hasText("timed out");
        }

        function test_check_now_reports_the_count() {
            root.answers.check = { code: 0, output: { version: 1, updates: 2, apps: [] } };
            loader.item.checkNow();
            hasText("2 updates found.");
        }
    }
}
