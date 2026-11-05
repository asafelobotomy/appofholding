pragma Singleton
import QtQuick

QtObject {
    // The backend answers with JSON text; failures look like {"error": "..."}.
    function parse(text) {
        try {
            return JSON.parse(text)
        } catch (e) {
            return {error: "unreadable answer from the backend"}
        }
    }

    function fmtSize(bytes) {
        if (!bytes && bytes !== 0) return ""
        var units = ["B", "KiB", "MiB", "GiB"]
        var v = bytes, i = 0
        while (v >= 1024 && i < units.length - 1) { v /= 1024; i++ }
        return (i === 0 ? v : v.toFixed(1)) + " " + units[i]
    }

    function fmtTime(unix) {
        if (!unix) return qsTr("never")
        return new Date(unix * 1000).toLocaleString(Qt.locale(), Locale.ShortFormat)
    }

    function statusLabel(status) {
        switch (status) {
        case "update-available": return qsTr("Update available")
        case "up-to-date": return qsTr("Up to date")
        case "unknown": return qsTr("Can't tell")
        default: return ""
        }
    }

    function sandboxLabel(level) {
        switch (level) {
        case "standard": return qsTr("Sandboxed")
        case "strict": return qsTr("Strict sandbox")
        case "custom": return qsTr("Custom sandbox")
        default: return ""
        }
    }
}
