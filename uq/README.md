# uQ integration

Files for the [uQ desktop](https://github.com/asafelobotomy/uq). They are inert without uQ:
nothing reads `/usr/share/uq/` unless uQ is installed. `make install` installs this folder
unconditionally, so the Arch, RPM and Debian packages all carry it.

| Path | Installed to | What it adds |
|---|---|---|
| `widgets/appofholding/` | `/usr/share/uq/shell/widgets/appofholding/` | A quick-settings tile, "AppImages · N updates"; a click opens the page below |
| `pages/appimages/` | `/usr/share/uq/settings/pages/appimages/` | **AppImages** in uQ Settings (needs uQ with page API 3): *Overview* (updates, Check for updates and Update all with progress, the apps with an update or a problem), *Apps* (each AppImage with its status, update policy, Update, Skip this version and Remove with a preview) and *Preferences* (background checks, previous versions kept, the policy and sandbox new apps start with, and whether oysterAV scans AppImages). With oysterAV, *Scan with oysterAV* on the Overview and in each app's menu, a mark on an app it flagged and a banner on the Overview |
| `launcher/actions/appofholding-check.appaction` | `/usr/share/uq/launcher/actions/` | **Check for update** on the app rows appOFholding made: runs `appofholding check --notify --desktop-id <id>` detached; the answer is a notification |
| `launcher/actions/appofholding-launch.appaction` | `/usr/share/uq/launcher/actions/` | **Launch options…** on those rows: opens appOFholding on that app's Launch tab (`appofholding-gui --tab launch --desktop-id <id>`) |

appOFholding's menu entries are `appofholding-<id>.desktop` (`paths::desktop_id`), so both
actions match `appofholding-*.desktop` and nothing else. Its AppImages already show in the
launcher through those entries; it adds no launcher results of its own.

Everything runs through declared commands, never a shell. The page reads these and shows
nothing else; the window of appOFholding does the same things through the same library.

- `appofholding list --json`: `{version: 1, updates, apps: [{id, name, version, desktop_id,
  appimage, size_bytes, icon, external, has_source, source, policy, status, latest, skipped,
  error, last_checked, update, scan}]}`. It reads the database only (the last check's result), never
  the network. `updates` counts the apps with an update that is neither skipped nor pinned.
- `appofholding scan [ID…|--all] --ndjson`: `{event: "start", total}`, `{event: "scanned", index, total, id,
  name, state, detail, at}` (`state` is `clean`, `threat` or `error`) and `{event: "done", threats, errors,
  total}`; the exit status is 1 when something was found. The verdicts are kept and show as `scan` in `list`.
- `appofholding check --ndjson`: asks every source now, saves the results, and prints one JSON
  line per step: `{event: "start", total}`, `{event: "checked", index, total, id, status,
  latest, problem, error, update}`, `{event: "done", updates, total}`. A source that fails is a
  `checked` line with `error`. (`check --json` prints one object at the end.)
- `appofholding update [ID] --defer-if-running --ndjson`: `{event: "start", app, name, index,
  total}`, `{event: "progress", app, percent}`, `{event: "result", app, name, outcome, from, to,
  message}` (`outcome` is `updated`, `current`, `unknown`, `skipped`, `deferred` or `failed`),
  and `{event: "done", updated, failed, total}`. Exit status 1 if any app failed.
- `appofholding update --dry-run --json` and `remove ID [--purge-data] --dry-run --json`: the
  plan uQ shows in its confirmation sheet, `{version: 1, ok, summary, changes: [{group, label,
  detail, bytes, reversible}], would_refuse, reason}`.
- `appofholding remove ID [--purge-data] --json`: `{version: 1, ok, id, name, kept_file,
  kept_data, purged_data}`.
- `appofholding config show --json`: `{version: 1, ok, settings: {policy, keep_versions,
  sandbox, schedule}, defaults, limits: {max_keep_versions}, apps_with_source}`; `config set
  KEY VALUE [--all] --json` and `config reset KEY --json` answer with the new `settings`, or
  `{version: 1, ok: false, error}` and exit status 1.

The page never passes `--force`, `--delete-file`, `--from` or anything that makes appOFholding
do more than its own refusals allow; `tests/uq/logic.test.mjs` checks that.

## Testing

- `uqctl integrations check uq/` (uQ 0.3.0 and later; CI builds it from a pinned uQ commit):
  clean apart from the programs it can't find where appOFholding and uQ aren't installed
  (`--no-programs`).
- `tests/uq/tst_widget.qml` and `tests/uq/tst_page.qml` load the widget and the page with a fake
  `ctx` and check what they show and which commands they run (CI runs them):

  ```
  QT_QPA_PLATFORM=offscreen qmltestrunner -input tests/uq/tst_widget.qml
  QT_QPA_PLATFORM=offscreen qmltestrunner -import tests/uq/stub -input tests/uq/tst_page.qml
  node --test 'tests/uq/*.test.mjs'     # logic.js and page.toml, without Qt
  ```

  `tests/uq/stub/` and `FakeContext.qml` are uQ's page test kit (`templates/settings-page-tests/`
  at the commit CI pins); refresh them when moving to a newer uQ.
- `cargo test` covers the JSON output, `--desktop-id`, and the GUI's `--app`, `--desktop-id` and
  `--tab` options.
