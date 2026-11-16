.pragma library
// The AppImages page's pure logic: no Qt calls, so Node can test it (tests/uq/logic.test.mjs).
// Everything `appofholding` prints is untrusted: it is read defensively, a wrong type becomes an
// empty or "couldn't read" result, lists are capped and strings are cut. The words are the
// ones the appOFholding window uses (crates/gui/qml), so both say the same thing.

var POLICY_KEYS = ["auto", "notify", "manual", "pinned"];
var POLICY_NAMES = ["Install it automatically", "Tell me", "Stay quiet, I'll update myself", "Never change the app"];
var SCHEDULE_KEYS = ["off", "hourly", "daily", "weekly"];
var SCHEDULE_NAMES = ["Off", "Every hour", "Every day", "Every week"];
var SANDBOX_KEYS = ["none", "standard", "strict"];
var SANDBOX_NAMES = ["Off", "Standard", "Strict"];
var SANDBOX_HELP = [
    "New apps run like any other program.",
    "New apps work normally but can't see your files.",
    "New apps can't see your files, and have no network or sound."
];

var SCAN_KEYS = ["off", "warn", "block"];
var SCAN_NAMES = ["Off", "Scan and warn me", "Scan and stop a threat"];
var SCAN_HELP = [
    "AppImages are not scanned unless you ask.",
    "Each AppImage is scanned as it is added or updated, and the answer is kept. A threat is only reported.",
    "As “Scan and warn me”, but an AppImage with a threat is not added or installed, and an update with one is thrown away."
];

var APP_LIMIT = 500;
var NAME_LIMIT = 200;
var MESSAGE_LIMIT = 400;
var RESULT_LIMIT = 200;

var APP_PATTERN = "^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$";
var CONFIG_KEYS = ["policy", "keep_versions", "sandbox", "schedule", "scan"];

/// The argument patterns of page.toml's [[command]] tables: command id -> argument -> pattern.
/// tests/uq/logic.test.mjs checks that they are the same strings.
var SHA_PATTERN = "^[0-9a-f]{12,64}$";
/// A web link or owner/repo, never a path.
var LINK_PATTERN = "^(https?://[^\\x00-\\x20\\x7f]+|[A-Za-z0-9][A-Za-z0-9._-]{0,99}/[A-Za-z0-9][A-Za-z0-9._-]{0,99})$";

var PATTERNS = {
    "preview-remove": { "app": APP_PATTERN },
    "preview-remove-data": { "app": APP_PATTERN },
    "update": { "app": APP_PATTERN },
    "policy": { "app": APP_PATTERN, "policy": "^(" + POLICY_KEYS.join("|") + ")$" },
    "skip": { "app": APP_PATTERN },
    "unskip": { "app": APP_PATTERN },
    "remove": { "app": APP_PATTERN },
    "remove-data": { "app": APP_PATTERN },
    "scan": { "app": APP_PATTERN },
    "versions": { "app": APP_PATTERN },
    "preview-rollback": { "to": SHA_PATTERN, "app": APP_PATTERN },
    "rollback": { "to": SHA_PATTERN, "app": APP_PATTERN },
    "add": { "link": LINK_PATTERN },
    "config-set": { "key": "^(" + CONFIG_KEYS.join("|") + ")$", "value": "^[a-z0-9]{1,16}$" },
    "config-policy-all": { "policy": "^(" + POLICY_KEYS.join("|") + ")$" }
};

function cut(text, limit) {
    const s = String(text);
    return s.length > limit ? s.slice(0, limit - 1) + "…" : s;
}

function plural(n, one, many) {
    return n + " " + (n === 1 ? one : many);
}

/// Why `args` can't be sent to `command`, or "" when they can. uQ checks the same patterns; this
/// shows the reason instead of a bare refusal.
function argsProblem(command, args) {
    const patterns = PATTERNS[command];
    if (!patterns)
        return "";
    const names = Object.keys(patterns);
    if (args.length !== names.length)
        return "Wrong number of arguments for " + command;
    for (let i = 0; i < names.length; i++) {
        if (typeof args[i] !== "string" || !new RegExp(patterns[names[i]]).test(args[i]))
            return "That " + names[i] + " can't be used here";
    }
    return "";
}

// ---- results

function isCancelled(result) {
    return !!result && result.cancelled === true;
}

/// A JSON command's answer as {ok, error, output}: the envelope when there is one, else the
/// transport error. `output` is whatever object came back.
function envelope(result) {
    if (!result)
        return { ok: false, error: "appofholding gave no answer", output: null };
    const out = result.output && typeof result.output === "object" ? result.output : null;
    if (out && out.ok === false)
        return { ok: false, error: cut(out.error || "appofholding refused", MESSAGE_LIMIT), output: out };
    if (result.error !== undefined)
        return { ok: false, error: cut(result.error, MESSAGE_LIMIT), output: out };
    return { ok: true, error: "", output: out };
}

/// What a text command (policy, skip) said, for the row under it: {message, failed}.
function textOutcome(result) {
    if (isCancelled(result))
        return null;
    if (!result || result.error !== undefined) {
        const detail = result && typeof result.output === "string" ? result.output.trim() : "";
        const why = cut(result && result.error !== undefined ? result.error : "appofholding gave no answer", MESSAGE_LIMIT);
        return { message: detail !== "" ? why + "\n" + cut(detail, MESSAGE_LIMIT) : why, failed: true };
    }
    const text = typeof result.output === "string" ? result.output.trim() : "";
    return { message: cut(text, MESSAGE_LIMIT), failed: false };
}

/// What a removal said: {message, failed}.
function removeOutcome(result) {
    if (isCancelled(result))
        return null;
    const env = envelope(result);
    if (!env.ok)
        return { message: env.error, failed: true };
    const o = env.output || {};
    let text = "Removed " + cut(o.name || o.id || "the app", NAME_LIMIT) + ".";
    if (typeof o.kept_file === "string" && o.kept_file !== "")
        text += " The AppImage file stays at " + cut(o.kept_file, MESSAGE_LIMIT) + ".";
    if (o.purged_data === true)
        text += " Its data was deleted.";
    else if (o.kept_data === true)
        text += " Its data was kept.";
    return { message: text, failed: false };
}

/// What going back said: {message, failed}.
function rollbackOutcome(result) {
    if (isCancelled(result))
        return null;
    const env = envelope(result);
    if (!env.ok)
        return { message: env.error, failed: true };
    const to = env.output && typeof env.output.to === "string" ? cut(env.output.to, 60) : "";
    return { message: "Went back" + (to !== "" ? " to " + to : "") + ". The version you left is kept, and won't be installed again by itself.", failed: false };
}

/// What adding from a link said: {message, failed}.
function addOutcome(result) {
    if (isCancelled(result))
        return null;
    const env = envelope(result);
    if (!env.ok)
        return { message: env.error, failed: true };
    const o = env.output || {};
    return { message: "Added " + cut(o.name || o.id || "the AppImage", NAME_LIMIT) + (typeof o.app_version === "string" ? " " + cut(o.app_version, 60) : "") + ".", failed: false };
}

/// `appofholding versions --json` as {kept: [{version, sha, sizeText}], problem}.
function versionsView(result) {
    const env = envelope(result);
    if (!env.ok)
        return { kept: [], problem: env.error };
    const list = env.output && Array.isArray(env.output.kept) ? env.output.kept : [];
    const kept = list.slice(0, 20).filter(v => v && typeof v.sha256 === "string" && new RegExp("^[0-9a-f]{64}$").test(v.sha256)).map(v => ({
        version: typeof v.version === "string" && v.version !== "" ? cut(v.version, 60) : "unknown version",
        sha: v.sha256.slice(0, 12),
        sizeText: sizeText(v.bytes)
    }));
    return { kept: kept, problem: "" };
}

function withNote(notes, key, note) {
    const next = Object.assign({}, notes);
    if (note === null || note === undefined)
        delete next[key];
    else
        next[key] = note;
    return next;
}

// ---- the library

/// What oysterAV last said about an app: {state, detail}, or null when it never did.
function scanOf(s) {
    if (!s || typeof s !== "object" || ["clean", "threat", "error"].indexOf(s.state) < 0)
        return null;
    return { state: s.state, detail: typeof s.detail === "string" ? cut(s.detail, MESSAGE_LIMIT) : "" };
}

/// The words for it ("" when the app was never scanned).
function scanText(app) {
    if (!app.scan)
        return "";
    if (app.scan.state === "clean")
        return "oysterAV found no threats.";
    if (app.scan.state === "threat")
        return "oysterAV found " + (app.scan.detail !== "" ? app.scan.detail : "a threat") + ". Don't run it; oysterAV can move it away.";
    return "oysterAV couldn't scan it" + (app.scan.detail !== "" ? ": " + app.scan.detail : ".");
}

/// How many apps oysterAV found a threat in.
function threatCount(apps) {
    return apps.filter(a => a.scan && a.scan.state === "threat").length;
}

/// `appofholding list --json` as {apps, updates, problem}. `problem` is "" when it read.
function libraryView(result) {
    const none = { apps: [], updates: 0, problem: "" };
    if (isCancelled(result) || (result && result.error === "already running"))
        return null;
    if (!result || result.error !== undefined)
        return Object.assign(none, { problem: cut(result ? result.error : "appofholding gave no answer", MESSAGE_LIMIT) });
    const out = result.output;
    if (!out || typeof out !== "object" || out.version !== 1 || !Array.isArray(out.apps))
        return Object.assign(none, { problem: "appofholding gave a list this page doesn't understand." });
    const apps = out.apps.slice(0, APP_LIMIT).filter(a => a && typeof a.id === "string" && new RegExp(APP_PATTERN).test(a.id)).map(a => ({
        id: a.id,
        name: cut(typeof a.name === "string" && a.name !== "" ? a.name : a.id, NAME_LIMIT),
        version: typeof a.version === "string" ? cut(a.version, 60) : "",
        icon: typeof a.icon === "string" ? a.icon : "",
        sizeBytes: typeof a.size_bytes === "number" ? a.size_bytes : 0,
        hasSource: a.has_source === true,
        policy: POLICY_KEYS.indexOf(a.policy) >= 0 ? a.policy : "",
        status: typeof a.status === "string" ? a.status : "",
        latest: typeof a.latest === "string" ? cut(a.latest, 60) : "",
        skipped: typeof a.skipped === "string" ? cut(a.skipped, 60) : "",
        error: typeof a.error === "string" ? cut(a.error, MESSAGE_LIMIT) : "",
        update: a.update === true,
        scan: scanOf(a.scan)
    }));
    return { apps: apps, updates: apps.filter(a => a.update).length, problem: "" };
}

/// How an app is doing, in the words of the window's cards: {text, tone}. The tone is "update",
/// "problem" or "plain".
function statusOf(app) {
    if (app.update)
        return { text: "Update available: " + app.latest, tone: "update" };
    if (app.error !== "")
        return { text: "Couldn't check for updates", tone: "problem" };
    if (!app.hasSource)
        return { text: "Updates are not set up", tone: "plain" };
    if (app.policy === "pinned")
        return { text: "Never changed (pinned)", tone: "plain" };
    if (app.status === "up-to-date")
        return { text: "Up to date", tone: "plain" };
    if (app.status === "update-available" && app.skipped !== "")
        return { text: "Skipping " + app.skipped, tone: "plain" };
    if (app.status === "unknown")
        return { text: "Can't tell if " + app.latest + " is newer", tone: "plain" };
    return { text: "Not checked yet", tone: "plain" };
}

/// The line under an app's name: its version, then how it is doing.
function subtitleOf(app) {
    const version = app.version !== "" ? app.version : "unknown version";
    return version + " · " + statusOf(app).text;
}

/// The groups the window's list uses, most urgent first: [{title, apps}], empty ones left out.
function groups(apps) {
    const by = (a, b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase());
    const updates = apps.filter(a => a.update).sort(by);
    const problems = apps.filter(a => !a.update && a.error !== "").sort(by);
    const rest = apps.filter(a => !a.update && a.error === "").sort(by);
    return [
        { title: "Updates available", apps: updates },
        { title: "Needs attention", apps: problems },
        { title: "Apps", apps: rest }
    ].filter(g => g.apps.length > 0);
}

function summary(view) {
    if (view === null)
        return "Reading your AppImages…";
    if (view.problem !== "")
        return "AppImages are unavailable";
    const n = view.apps.length;
    if (n === 0)
        return "No AppImages yet";
    if (view.updates === 0)
        return n === 1 ? "1 AppImage, up to date" : n + " AppImages, all up to date";
    return plural(view.updates, "update", "updates") + " for " + plural(n, "AppImage", "AppImages");
}

function sizeText(bytes) {
    if (typeof bytes !== "number" || bytes <= 0)
        return "";
    const units = ["B", "KB", "MB", "GB"];
    let v = bytes, i = 0;
    while (v >= 1000 && i < units.length - 1) {
        v /= 1000;
        i++;
    }
    return (i === 0 ? String(v) : v.toFixed(1)) + " " + units[i];
}

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

// ---- settings

/// `appofholding config show --json` as {settings, defaults, maxKeep, withSource, problem}.
function configView(result) {
    const empty = { settings: null, defaults: null, maxKeep: 10, withSource: 0, problem: "" };
    if (isCancelled(result) || (result && result.error === "already running"))
        return null;
    const env = envelope(result);
    const out = env.output;
    if (!env.ok || !out || out.version !== 1 || !out.settings || typeof out.settings !== "object")
        return Object.assign(empty, { problem: env.ok ? "appofholding gave settings this page doesn't understand." : env.error });
    const s = out.settings;
    const keep = typeof s.keep_versions === "number" ? s.keep_versions : 2;
    return {
        settings: {
            policy: POLICY_KEYS.indexOf(s.policy) >= 0 ? s.policy : "notify",
            keep_versions: keep,
            sandbox: SANDBOX_KEYS.indexOf(s.sandbox) >= 0 ? s.sandbox : "none",
            schedule: SCHEDULE_KEYS.indexOf(s.schedule) >= 0 ? s.schedule : "off",
            scan: SCAN_KEYS.indexOf(s.scan) >= 0 ? s.scan : "off"
        },
        maxKeep: out.limits && typeof out.limits.max_keep_versions === "number" ? out.limits.max_keep_versions : 10,
        withSource: typeof out.apps_with_source === "number" ? out.apps_with_source : 0,
        problem: ""
    };
}

function indexOfKey(keys, key, fallback) {
    const i = keys.indexOf(key);
    return i >= 0 ? i : fallback;
}
