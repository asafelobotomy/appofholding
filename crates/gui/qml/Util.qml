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

    // "5 minutes ago", "2 hours ago", "3 days ago"
    function ago(unix) {
        if (!unix) return qsTr("never checked")
        var s = Math.max(0, Math.floor(Date.now() / 1000) - unix)
        if (s < 90) return qsTr("just now")
        if (s < 3600) return qsTr("%1 minutes ago").arg(Math.round(s / 60))
        if (s < 86400) return qsTr("%1 hours ago").arg(Math.round(s / 3600))
        return qsTr("%1 days ago").arg(Math.round(s / 86400))
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
