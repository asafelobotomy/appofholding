.pragma library
.import "logic.js" as Logic

// The progress of a check or an update that is running, read from its NDJSON events.
// Pure, like logic.js (tests/uq/logic.test.mjs).

var NAME_LIMIT = Logic.NAME_LIMIT;
var MESSAGE_LIMIT = Logic.MESSAGE_LIMIT;
var RESULT_LIMIT = Logic.RESULT_LIMIT;
var cut = Logic.cut;
var plural = Logic.plural;
var isCancelled = Logic.isCancelled;

// ---- progress

/// The state of a check or an update that is running, from its events (`appofholding check
/// --ndjson`, `update --ndjson`). Pure: each event gives the next state.
///   {kind, total, index, name, percent, results: [{app, name, outcome, text}], done, summary}
function startProgress(kind) {
    return { kind: kind, total: 0, index: 0, name: "", percent: -1, results: [], done: false, summary: "" };
}

function resultText(ev) {
    const name = cut(ev.name || ev.app || "", NAME_LIMIT);
    switch (ev.outcome) {
    case "updated":
        return name + ": updated" + (ev.from ? " from " + cut(ev.from, 60) : "") + (ev.to ? " to " + cut(ev.to, 60) : "");
    case "current":
        return name + ": already up to date";
    case "unknown":
        return name + ": can't tell if there is an update";
    case "skipped":
        return name + ": skipped" + (ev.message ? " (" + cut(ev.message, MESSAGE_LIMIT) + ")" : "");
    case "deferred":
        return name + ": " + (ev.to ? cut(ev.to, 60) + " is ready" : "an update is ready") + ", but the app is running. Close it and try again.";
    case "failed":
        return name + ": " + cut(ev.message || "failed", MESSAGE_LIMIT);
    }
    return name + ": " + cut(ev.outcome || "done", 60);
}

function reduce(state, ev) {
    if (!ev || typeof ev !== "object")
        return state;
    const next = Object.assign({}, state, { results: state.results.slice() });
    switch (ev.event) {
    case "start":
        if (typeof ev.total === "number")
            next.total = ev.total;
        if (typeof ev.index === "number")
            next.index = ev.index;
        if (typeof ev.name === "string")
            next.name = cut(ev.name, NAME_LIMIT);
        next.percent = -1;
        break;
    case "progress":
        if (typeof ev.percent === "number")
            next.percent = Math.max(0, Math.min(100, ev.percent));
        break;
    case "checked":
        if (typeof ev.total === "number")
            next.total = ev.total;
        if (typeof ev.index === "number")
            next.index = ev.index;
        next.name = cut(ev.id || "", NAME_LIMIT);
        if (next.results.length < RESULT_LIMIT && (ev.update === true || (typeof ev.error === "string" && ev.error !== ""))) {
            next.results.push({
                app: String(ev.id || ""),
                name: String(ev.id || ""),
                outcome: ev.update === true ? "update" : "failed",
                text: ev.update === true ? String(ev.id) + ": " + cut(ev.latest || "an update", 60) + " is available"
                    : String(ev.id) + ": " + cut(ev.error, MESSAGE_LIMIT)
            });
        }
        break;
    case "scanned":
        if (typeof ev.total === "number")
            next.total = ev.total;
        if (typeof ev.index === "number")
            next.index = ev.index;
        next.name = cut(ev.name || ev.id || "", NAME_LIMIT);
        if (next.results.length < RESULT_LIMIT && (ev.state === "threat" || ev.state === "error")) {
            const name = cut(ev.name || ev.id || "", NAME_LIMIT);
            next.results.push({
                app: String(ev.id || ""),
                name: name,
                outcome: "failed",
                text: ev.state === "threat" ? name + ": oysterAV found " + cut(ev.detail || "a threat", MESSAGE_LIMIT) : name + ": couldn't be scanned (" + cut(ev.detail || "unknown reason", MESSAGE_LIMIT) + ")"
            });
        }
        break;
    case "result":
        if (next.results.length < RESULT_LIMIT)
            next.results.push({ app: String(ev.app || ""), name: cut(ev.name || ev.app || "", NAME_LIMIT), outcome: String(ev.outcome || ""), text: resultText(ev) });
        next.percent = -1;
        break;
    case "done":
        next.done = true;
        next.summary = doneSummary(state.kind, ev);
        break;
    }
    return next;
}

function doneSummary(kind, ev) {
    if (kind === "scan") {
        const threats = typeof ev.threats === "number" ? ev.threats : 0;
        const errors = typeof ev.errors === "number" ? ev.errors : 0;
        const parts = [threats === 0 ? "No threats found" : plural(threats, "threat", "threats") + " found"];
        if (errors > 0)
            parts.push(plural(errors, "couldn't be scanned", "couldn't be scanned"));
        return parts.join(", ") + ".";
    }
    if (kind === "check") {
        const n = typeof ev.updates === "number" ? ev.updates : 0;
        return n === 0 ? "No updates found." : plural(n, "update", "updates") + " found.";
    }
    const updated = typeof ev.updated === "number" ? ev.updated : 0;
    const failed = typeof ev.failed === "number" ? ev.failed : 0;
    const parts = [];
    parts.push(updated === 0 ? "Nothing was updated" : plural(updated, "AppImage", "AppImages") + " updated");
    if (failed > 0)
        parts.push(plural(failed, "failed", "failed"));
    return parts.join(", ") + ".";
}

/// 0..1 for a progress bar: the finished apps plus the part of this one downloaded; -1 when it
/// can't say (the bar then shows it is busy).
function fraction(state) {
    if (state.total <= 0)
        return -1;
    if (state.kind === "check" || state.kind === "scan")
        return Math.min(1, Math.max(0, state.index / state.total));
    const base = Math.max(0, state.index - 1) / state.total;
    const part = state.percent >= 0 ? state.percent / 100 / state.total : 0;
    return Math.min(1, base + part);
}

function progressLine(state) {
    if (state.done)
        return state.summary;
    if (state.kind === "scan" && (state.total <= 0 || state.name === ""))
        return "Asking oysterAV…";
    const verb = state.kind === "scan" ? "Scanned " : state.kind === "check" ? "Checking " : (state.percent >= 0 ? "Downloading " : "Updating ");
    if (state.total <= 0 || state.name === "")
        return state.kind === "check" ? "Asking update sources…" : "Starting…";
    const where = state.total > 1 ? " (" + Math.max(1, state.index) + " of " + state.total + ")" : "";
    const pct = state.percent >= 0 ? ", " + Math.round(state.percent) + "%" : "";
    return verb + state.name + where + pct;
}

/// What a finished or failed stream says when the stream itself ended with an error that the
/// events did not explain: "" when the events told the story.
function streamProblem(state, result) {
    if (isCancelled(result))
        return "";
    if (state.done)
        return "";
    if (result && result.error !== undefined)
        return cut(result.error, MESSAGE_LIMIT);
    return "";
}
