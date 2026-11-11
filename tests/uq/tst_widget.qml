// Loads the uQ quick-settings widget (uq/widgets/appofholding) with a fake ctx, as uQ's shell
// would, and checks the tile text and the commands it runs. Run with:
//   QT_QPA_PLATFORM=offscreen qmltestrunner -input tests/uq/tst_widget.qml
import QtQuick
import QtTest

Item {
    id: root
    width: 300; height: 32

    property var calls: []
    property var answers: ({})
    property var theme: ({ text: "white", textMuted: "gray", accent: "blue", overlay: "#333",
                           surfaceAlt: "#222", radius: 10 })
    Component { id: fakeText; Text { textFormat: Text.PlainText } }

    property var ctx: ({
        Theme: theme, theme: theme, UqText: fakeText, UqInput: null, slot: "quick",
        command: { run: function (id, args, cb) {
            root.calls.push({ id: id, args: args });
            var a = root.answers[id] || { code: 0, output: null };
            Qt.callLater(function () { cb(a); });
        } },
        openPopup: function () {}, closePopup: function () {},
        fail: function (r) { throw new Error("failed: " + r); }
    })

    Loader {
        id: tile
        width: 300; height: 32
        source: Qt.resolvedUrl("../../uq/widgets/appofholding/Widget.qml")
        onLoaded: item.ctx = root.ctx
    }

    TestCase {
        name: "AppofholdingWidget"
        when: windowShown

        function shown(item, out) {
            out = out || [];
            if (typeof item.text === "string" && item.visible) out.push(item.text);
            for (var i = 0; i < item.children.length; i++) shown(item.children[i], out);
            return out;
        }

        function show(answer, expected) {
            root.answers = { list: answer };
            tile.item.refresh();
            tryVerify(function () { return shown(tile.item).indexOf(expected) >= 0; }, 2000,
                      expected + " in " + JSON.stringify(shown(tile.item)));
        }

        function test_counts_and_opens_the_page() {
            show({ code: 0, output: { version: 1, apps: [{ id: "a" }, { id: "b" }, { id: "c" }], updates: 2 } },
                 "AppImages · 2 updates");
            show({ code: 0, output: { version: 1, apps: [{ id: "a" }], updates: 1 } }, "AppImages · 1 update");
            show({ code: 0, output: { version: 1, apps: [{ id: "a" }], updates: 0 } }, "AppImages · up to date");
            show({ code: 0, output: { version: 1, apps: [], updates: 0 } }, "AppImages · none yet");
            show({ error: "timed out" }, "AppImages · unavailable");

            root.calls = [];
            mouseClick(tile.item, 50, 16);
            compare(root.calls.length, 1);
            compare(root.calls[0].id, "open-page");
            compare(root.calls[0].args, []);
        }
    }
}
