// Tests for the AppImages page in uQ Settings (uq/pages/appimages/): its logic.js, its manifest,
// and the way its QML uses the commands.   node --test 'tests/uq/*.test.mjs'
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { loadLogic } from "./load-logic.mjs";
import { parseToml } from "./toml-lite.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const pageDir = join(here, "..", "..", "uq", "pages", "appimages");
const L = { ...loadLogic(join(pageDir, "logic.js")), ...loadLogic(join(pageDir, "progress.js")) };
const manifest = parseToml(readFileSync(join(pageDir, "page.toml"), "utf8"));
const qmlFiles = readdirSync(pageDir).filter((f) => f.endsWith(".qml"));
const read = (f) => readFileSync(join(pageDir, f), "utf8");
const command = (id) => manifest.command.find((c) => c.id === id);

// ---- manifest -------------------------------------------------------------------------------

test("the page uses uq_api 3 and every command it declares is used", () => {
  assert.equal(manifest.uq_api, 3);
  assert.equal(manifest.id, "appimages");
  const qml = qmlFiles.map(read).join("\n") + read("logic.js") + read("progress.js");
  const previews = new Set(manifest.command.map((c) => c.preview).filter(Boolean));
  for (const c of manifest.command) {
    if (previews.has(c.id)) continue; // run by uQ itself, before the confirmation
    assert.ok(qml.includes(`"${c.id}"`), `command ${c.id} is never used`);
  }
});

test("every command the QML runs is declared, and streams are ndjson", () => {
  const ids = new Set(manifest.command.map((c) => c.id));
  for (const f of qmlFiles) {
    for (const m of read(f).matchAll(/(?:\.write|\.stream|command\.run|command\.watch)\(\s*(?:"[^"]*",\s*)?"([a-z-]+)"/g)) {
      assert.ok(ids.has(m[1]), `${f} uses undeclared command ${m[1]}`);
    }
  }
  for (const id of ["check", "update-all", "update"]) assert.equal(command(id).output, "ndjson", id);
  for (const c of manifest.command) if (c.output === "ndjson") assert.ok(c.argv.includes("--ndjson"), `${c.id} has no --ndjson`);
});

test("a command with a preview confirms, and its preview is a read of the same page", () => {
  for (const c of manifest.command.filter((c) => c.preview)) {
    assert.ok(c.confirm, `${c.id} has a preview but no confirm`);
    const p = command(c.preview);
    assert.ok(p, `${c.id}: no command ${c.preview}`);
    assert.equal(p.output, "json");
    assert.ok(!p.confirm && !p.preview);
    assert.ok(p.argv.includes("--dry-run"), `${p.id} is not a dry run`);
    assert.deepEqual((p.args ?? []).map((a) => a.pattern), (c.args ?? []).map((a) => a.pattern), `${c.id}: preview takes other arguments`);
  }
  for (const id of ["update-all", "remove", "remove-data"]) assert.ok(command(id).preview, id);
});

test("tst_page.qml lists the commands that confirm", () => {
  const plan = manifest.command.filter((c) => c.confirm).map((c) => c.id).sort();
  const test = readFileSync(join(here, "tst_page.qml"), "utf8");
  const listed = /confirmed:\s*\[([^\]]*)\]/.exec(test);
  assert.ok(listed);
  assert.deepEqual([...listed[1].matchAll(/"([a-z-]+)"/g)].map((m) => m[1]).sort(), plan);
});

test("the page never forces or deletes an AppImage file", () => {
  for (const c of manifest.command) {
    for (const flag of ["--force", "--delete-file", "--from", "--yes"]) assert.ok(!c.argv.includes(flag), `${c.id} passes ${flag}`);
  }
});

test("argument patterns are the same in logic.js and page.toml", () => {
  let seen = 0;
  for (const c of manifest.command) {
    for (const a of c.args ?? []) {
      seen++;
      assert.equal(a.pattern, L.PATTERNS[c.id]?.[a.name], `${c.id}.${a.name}`);
    }
    assert.equal((c.args ?? []).length, Object.keys(L.PATTERNS[c.id] ?? {}).length, `${c.id}: argument count`);
  }
  assert.equal(seen, 19);
});

const HOSTILE = ["-rf", "--purge-data", "a\nb", "", "x".repeat(200), "../x", "$(id)", "-", "a b", "A\u0000"];
test("hostile arguments are refused by logic.js exactly as uQ would", () => {
  const uqAccepts = (pattern, v) => v !== "" && v[0] !== "-" && Buffer.byteLength(v) <= 4096 && new RegExp(pattern).test(v);
  for (const [id, args] of Object.entries(L.PATTERNS)) {
    const names = Object.keys(args);
    for (const [i, name] of names.entries()) {
      for (const bad of HOSTILE) {
        const given = names.map((n, j) => (j === i ? bad : n === "policy" ? "auto" : n === "key" ? "policy" : n === "value" ? "auto" : n === "to" ? "0123456789ab" : n === "link" ? "owner/repo" : "krita"));
        assert.equal(L.argsProblem(id, given) === "", uqAccepts(args[name], bad), `${id}.${name} ${JSON.stringify(bad.slice(0, 12))}`);
      }
    }
  }
  assert.equal(L.argsProblem("remove", ["krita"]), "");
  assert.equal(L.argsProblem("remove", ["--purge-data"]) === "", false);
  assert.equal(L.argsProblem("remove", []) === "", false);
  assert.equal(L.argsProblem("config-set", ["keep_versions", "4"]), "");
  assert.equal(L.argsProblem("config-set", ["colour", "red"]) === "", false);
});

// ---- the page's text ------------------------------------------------------------------------

const BANNED = /D-?Bus|polkit|JSON|systemctl|sqlite|ndjson|\borg\.[a-z0-9_]+\.[A-Za-z0-9_.]+/i;
test("the page's words are plain", () => {
  const found = [];
  const literals = (line) => [...line.matchAll(/"((?:[^"\\]|\\.)*)"/g)].map((m) => m[1]);
  for (const f of qmlFiles) for (const line of read(f).split("\n")) if (/\btext:|title:|detail:|help:|label:|placeholderText:/.test(line)) found.push(...literals(line));
  for (const line of (read("logic.js") + "\n" + read("progress.js")).split("\n")) {
    if (/^\s*\/\//.test(line) || /PATTERN|pattern|RegExp|\.test\(|case |\.event|ev\./.test(line)) continue;
    found.push(...literals(line).filter((s) => /^[A-Z].* /.test(s) || /…/.test(s)));
  }
  found.push(manifest.name, ...manifest.setting.map((s) => s.label), ...manifest.command.filter((c) => c.confirm).map((c) => c.confirm));
  assert.ok(found.length > 40, `only ${found.length} strings`);
  for (const s of found) assert.ok(!BANNED.test(s), `jargon in: ${s}`);
});

// ---- reading what appofholding prints -------------------------------------------------------

const app = (o) => ({ id: "krita", name: "Krita", version: "5.2.0", icon: "appofholding-krita", size_bytes: 2500000, has_source: true, policy: "notify", status: "up-to-date", latest: "5.2.0", skipped: null, error: null, update: false, ...o });
const list = (apps) => ({ code: 0, output: { version: 1, updates: apps.filter((a) => a && a.update).length, apps } });

test("libraryView reads the list and drops what it can't trust", () => {
  const v = L.libraryView(list([app({}), app({ id: "tool", name: "Tool", update: true, latest: "2.0", status: "update-available" }), app({ id: "-rf" }), null, app({ id: "x".repeat(200) })]));
  assert.equal(v.problem, "");
  assert.deepEqual(v.apps.map((a) => a.id), ["krita", "tool"]);
  assert.equal(v.updates, 1);
  assert.equal(L.libraryView({ error: "boom" }).problem, "boom");
  assert.match(L.libraryView({ code: 0, output: { version: 2, apps: [] } }).problem, /doesn't understand/);
  assert.match(L.libraryView({ code: 0, output: "nope" }).problem, /doesn't understand/);
  assert.equal(L.libraryView({ error: "Cancelled", cancelled: true }), null);
  assert.equal(L.libraryView({ error: "already running" }), null);
  const long = L.libraryView(list([app({ name: "N".repeat(1000), error: "E".repeat(5000) })])).apps[0];
  assert.ok(long.name.length <= 200 && long.error.length <= 400);
  const many = L.libraryView(list(Array.from({ length: 900 }, (_, i) => app({ id: `a${i}` }))));
  assert.equal(many.apps.length, 500);
});

test("statusOf uses the window's words", () => {
  const view = (o) => L.libraryView(list([app(o)])).apps[0];
  assert.deepEqual(L.statusOf(view({ update: true, latest: "5.3" })), { text: "Update available: 5.3", tone: "update" });
  assert.equal(L.statusOf(view({ error: "HTTP 404" })).text, "Couldn't check for updates");
  assert.equal(L.statusOf(view({ has_source: false, policy: null, status: null })).text, "Updates are not set up");
  assert.equal(L.statusOf(view({})).text, "Up to date");
  assert.equal(L.statusOf(view({ status: null })).text, "Not checked yet");
  assert.equal(L.statusOf(view({ policy: "pinned" })).text, "Never changed (pinned)");
  assert.equal(L.statusOf(view({ status: "update-available", latest: "6", skipped: "6" })).text, "Skipping 6");
  assert.equal(L.subtitleOf(view({ version: null })), "unknown version · Up to date");
});

test("groups put updates first, then problems, then the rest by name", () => {
  const apps = L.libraryView(list([app({ id: "z", name: "Zed" }), app({ id: "b", name: "Bad", error: "x" }), app({ id: "u", name: "Up", update: true, latest: "2" }), app({ id: "a", name: "alpha" })])).apps;
  const g = L.groups(apps);
  assert.deepEqual(g.map((x) => x.title), ["Updates available", "Needs attention", "Apps"]);
  assert.deepEqual(g[2].apps.map((a) => a.id), ["a", "z"]);
  assert.deepEqual(L.groups([]), []);
});

test("summary and sizes read plainly", () => {
  assert.equal(L.summary(null), "Reading your AppImages…");
  assert.equal(L.summary({ apps: [], updates: 0, problem: "x" }), "AppImages are unavailable");
  assert.equal(L.summary({ apps: [], updates: 0, problem: "" }), "No AppImages yet");
  assert.equal(L.summary({ apps: [1], updates: 0, problem: "" }), "1 AppImage, up to date");
  assert.equal(L.summary({ apps: [1, 2], updates: 0, problem: "" }), "2 AppImages, all up to date");
  assert.equal(L.summary({ apps: [1, 2, 3], updates: 1, problem: "" }), "1 update for 3 AppImages");
  assert.equal(L.sizeText(1500000), "1.5 MB");
  assert.equal(L.sizeText(900), "900 B");
  assert.equal(L.sizeText(0), "");
});

// ---- progress -------------------------------------------------------------------------------

test("an update's events become a bar, a line and results", () => {
  let s = L.startProgress("update");
  assert.equal(L.progressLine(s), "Starting…");
  assert.equal(L.fraction(s), -1);
  s = L.reduce(s, { event: "start", app: "a", name: "Alpha", index: 1, total: 2 });
  assert.equal(L.progressLine(s), "Updating Alpha (1 of 2)");
  s = L.reduce(s, { event: "progress", percent: 50 });
  assert.equal(L.progressLine(s), "Downloading Alpha (1 of 2), 50%");
  assert.equal(L.fraction(s), 0.25);
  s = L.reduce(s, { event: "result", app: "a", name: "Alpha", outcome: "updated", from: "1", to: "2" });
  s = L.reduce(s, { event: "start", app: "b", name: "Beta", index: 2, total: 2 });
  assert.equal(L.fraction(s), 0.5);
  s = L.reduce(s, { event: "result", app: "b", name: "Beta", outcome: "failed", message: "no network" });
  s = L.reduce(s, { event: "done", updated: 1, failed: 1 });
  assert.equal(s.done, true);
  assert.equal(L.progressLine(s), "1 AppImage updated, 1 failed.");
  assert.deepEqual(s.results.map((r) => r.text), ["Alpha: updated from 1 to 2", "Beta: no network"]);
  assert.equal(L.reduce(s, "junk"), s);
  assert.equal(L.reduce(s, null), s);
});

test("a check's events count updates and keep only what needs saying", () => {
  let s = L.startProgress("check");
  assert.equal(L.progressLine(s), "Asking update sources…");
  s = L.reduce(s, { event: "start", total: 3 });
  s = L.reduce(s, { event: "checked", index: 1, total: 3, id: "a", update: false, latest: "1", error: null });
  s = L.reduce(s, { event: "checked", index: 2, total: 3, id: "b", update: true, latest: "2", error: null });
  s = L.reduce(s, { event: "checked", index: 3, total: 3, id: "c", update: false, latest: null, error: "HTTP 500" });
  assert.equal(L.fraction(s), 1);
  s = L.reduce(s, { event: "done", updates: 1, total: 3 });
  assert.equal(s.summary, "1 update found.");
  assert.deepEqual(s.results.map((r) => r.text), ["b: 2 is available", "c: HTTP 500"]);
  assert.equal(L.reduce(L.startProgress("check"), { event: "done", updates: 0 }).summary, "No updates found.");
});

test("a deferred or skipped app is said plainly, and results are capped", () => {
  assert.match(L.resultText({ app: "a", name: "A", outcome: "deferred", to: "2" }), /2 is ready, but the app is running/);
  assert.match(L.resultText({ app: "a", name: "A", outcome: "skipped", message: "pinned" }), /skipped \(pinned\)/);
  assert.match(L.resultText({ app: "a", name: "A", outcome: "current" }), /already up to date/);
  let s = L.startProgress("update");
  for (let i = 0; i < 300; i++) s = L.reduce(s, { event: "result", app: "a", name: "A", outcome: "current" });
  assert.equal(s.results.length, 200);
  s = L.reduce(s, { event: "progress", percent: 900 });
  assert.equal(s.percent, 100);
});

test("a stream that ended badly without saying why is reported; one that said done is not", () => {
  const s = L.startProgress("update");
  assert.equal(L.streamProblem(s, { error: "exit 1: boom" }), "exit 1: boom");
  assert.equal(L.streamProblem(L.reduce(s, { event: "done", updated: 0, failed: 1 }), { error: "exit 1" }), "");
  assert.equal(L.streamProblem(s, { error: "Cancelled", cancelled: true }), "");
});

// ---- answers --------------------------------------------------------------------------------

test("envelope, removal and text outcomes", () => {
  assert.equal(L.envelope({ code: 0, output: { ok: true } }).ok, true);
  assert.deepEqual(L.envelope({ code: 1, error: "exit 1", output: { ok: false, error: "no such app" } }), { ok: false, error: "no such app", output: { ok: false, error: "no such app" } });
  assert.equal(L.envelope({ error: "timed out" }).error, "timed out");
  assert.equal(L.envelope(null).ok, false);
  assert.equal(L.removeOutcome({ error: "Cancelled", cancelled: true }), null);
  assert.equal(L.removeOutcome({ code: 0, output: { ok: true, name: "Tool", kept_file: "/home/u/Tool.AppImage", kept_data: true, purged_data: false } }).message, "Removed Tool. The AppImage file stays at /home/u/Tool.AppImage. Its data was kept.");
  assert.equal(L.removeOutcome({ code: 1, error: "exit 1", output: { ok: false, error: "no such app" } }).failed, true);
  assert.deepEqual(L.textOutcome({ code: 0, output: "krita: update policy is now auto\n" }), { message: "krita: update policy is now auto", failed: false });
  assert.equal(L.textOutcome({ code: 1, error: "exit 1: nope", output: "krita: bad\n" }).failed, true);
  assert.equal(L.textOutcome({ error: "Cancelled", cancelled: true }), null);
  assert.deepEqual(L.withNote({ a: 1 }, "b", { x: 1 }), { a: 1, b: { x: 1 } });
  assert.deepEqual(L.withNote({ a: 1 }, "a", null), {});
});

test("configView reads the settings and clamps what it can't trust", () => {
  const v = L.configView({ code: 0, output: { version: 1, ok: true, settings: { policy: "auto", keep_versions: 4, sandbox: "strict", schedule: "weekly", scan: "block" }, limits: { max_keep_versions: 10 }, apps_with_source: 3 } });
  assert.deepEqual(v.settings, { policy: "auto", keep_versions: 4, sandbox: "strict", schedule: "weekly", scan: "block" });
  assert.equal(v.withSource, 3);
  const odd = L.configView({ code: 0, output: { version: 1, ok: true, settings: { policy: "wild", sandbox: 3, schedule: "monthly", scan: "loud" } } });
  assert.deepEqual(odd.settings, { policy: "notify", keep_versions: 2, sandbox: "none", schedule: "off", scan: "off" });
  assert.equal(L.configView({ error: "no" }).problem, "no");
  assert.match(L.configView({ code: 0, output: { version: 1, ok: true } }).problem, /doesn't understand/);
  assert.equal(L.configView({ error: "Cancelled", cancelled: true }), null);
  assert.equal(L.indexOfKey(L.POLICY_KEYS, "manual", 1), 2);
  assert.equal(L.indexOfKey(L.POLICY_KEYS, "zzz", 1), 1);
});

test("add takes a link or owner/repo, never a path; rollback takes a hash", () => {
  for (const ok of ["owner/repo", "https://github.com/o/r", "http://example.com/a/b.AppImage?x=1"]) assert.equal(L.argsProblem("add", [ok]), "", ok);
  for (const bad of ["/etc/passwd", "./x", "../x", "~/x", "file:///x", "a b", "owner/repo/extra", "https://x/ y"]) assert.notEqual(L.argsProblem("add", [bad]), "", bad);
  assert.equal(L.argsProblem("rollback", ["0123456789ab", "krita"]), "");
  assert.notEqual(L.argsProblem("rollback", ["1.0.0", "krita"]), "");
  assert.notEqual(L.argsProblem("rollback", ["krita", "0123456789ab"]), "");
});

test("versions, going back and adding are worded plainly", () => {
  const sha = "0123456789abcdef".repeat(4);
  const v = L.versionsView({ code: 0, output: { version: 1, ok: true, kept: [{ version: "1.0", sha256: sha, bytes: 2500000 }, { version: null, sha256: sha, bytes: 0 }, { sha256: "zz" }, null] } });
  assert.deepEqual(v.kept, [{ version: "1.0", sha: sha.slice(0, 12), sizeText: "2.5 MB" }, { version: "unknown version", sha: sha.slice(0, 12), sizeText: "" }]);
  assert.equal(L.versionsView({ error: "boom" }).problem, "boom");
  assert.equal(L.rollbackOutcome({ code: 0, output: { ok: true, to: "1.0" } }).message, "Went back to 1.0. The version you left is kept, and won't be installed again by itself.");
  assert.equal(L.rollbackOutcome({ error: "Cancelled", cancelled: true }), null);
  assert.equal(L.rollbackOutcome({ code: 1, error: "exit 1", output: { ok: false, error: "no kept version" } }).failed, true);
  assert.equal(L.addOutcome({ code: 0, output: { ok: true, name: "Krita", app_version: "5.2" } }).message, "Added Krita 5.2.");
  assert.equal(L.addOutcome({ code: 1, error: "exit 1", output: { ok: false, error: "network error" } }).message, "network error");
});

test("choice lists line up with their keys", () => {
  assert.equal(L.POLICY_KEYS.length, L.POLICY_NAMES.length);
  assert.equal(L.SCHEDULE_KEYS.length, L.SCHEDULE_NAMES.length);
  assert.equal(L.SANDBOX_KEYS.length, L.SANDBOX_NAMES.length);
  assert.equal(L.SANDBOX_KEYS.length, L.SANDBOX_HELP.length);
  assert.equal(L.SCAN_KEYS.length, L.SCAN_NAMES.length);
  assert.equal(L.SCAN_KEYS.length, L.SCAN_HELP.length);
});

test("oysterAV: what the list says is read defensively, and the words never hide a threat", () => {
  const row = (scan) => L.libraryView({ code: 0, output: { version: 1, apps: [{ id: "foo", name: "Foo", scan }], updates: 0 } }).apps[0];
  assert.equal(row(null).scan, null);
  assert.equal(row({ state: "weird", detail: "x" }).scan, null);
  assert.equal(L.scanText(row(null)), "");
  assert.equal(L.scanText(row({ state: "clean", detail: "" })), "oysterAV found no threats.");
  assert.match(L.scanText(row({ state: "threat", detail: "Win.Test" })), /oysterAV found Win\.Test\. Don't run it/);
  assert.match(L.scanText(row({ state: "error", detail: "oysterAV is not installed" })), /couldn't scan it: oysterAV is not installed/);
  assert.equal(L.threatCount([row({ state: "threat", detail: "" }), row({ state: "clean", detail: "" }), row(null)]), 1);
});

test("oysterAV: a scan streams one line per app and ends with the totals", () => {
  let st = L.startProgress("scan");
  assert.equal(L.progressLine(st), "Asking oysterAV…");
  st = L.reduce(st, { event: "start", total: 2 });
  st = L.reduce(st, { event: "scanned", index: 1, total: 2, id: "foo", name: "Foo", state: "clean", detail: "" });
  assert.equal(st.results.length, 0, "clean apps are not listed");
  assert.equal(L.fraction(st), 0.5);
  st = L.reduce(st, { event: "scanned", index: 2, total: 2, id: "bar", name: "Bar", state: "threat", detail: "Win.Test" });
  assert.equal(st.results[0].text, "Bar: oysterAV found Win.Test");
  assert.equal(st.results[0].outcome, "failed");
  st = L.reduce(st, { event: "done", threats: 1, errors: 0, total: 2 });
  assert.equal(L.progressLine(st), "1 threat found.");
  assert.equal(L.reduce(L.startProgress("scan"), { event: "done", threats: 0, errors: 2, total: 2 }).summary, "No threats found, 2 couldn't be scanned.");
});
