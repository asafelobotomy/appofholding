# uQ integration

Files for the [uQ desktop](https://github.com/asafelobotomy/uq). They are inert without uQ:
nothing reads `/usr/share/uq/` unless uQ is installed. `make install` installs this folder
unconditionally, so the Arch, RPM and Debian packages all carry it.

| Path | Installed to | What it adds |
|---|---|---|
| `widgets/appofholding/` | `/usr/share/uq/shell/widgets/appofholding/` | A quick-settings tile, "AppImages · N updates"; a click opens the page below |
| `pages/appimages/` | `/usr/share/uq/settings/pages/appimages/` | **AppImages** in uQ Settings: the library with each app's status, Check now, Update all (or one app), each app's update policy, and the background check schedule |
| `launcher/actions/appofholding-check.appaction` | `/usr/share/uq/launcher/actions/` | **Check for update** on the app rows appOFholding made: runs `appofholding check --notify --desktop-id <id>` detached; the answer is a notification |
| `launcher/actions/appofholding-launch.appaction` | `/usr/share/uq/launcher/actions/` | **Launch options…** on those rows: opens appOFholding on that app's Launch tab (`appofholding-gui --tab launch --desktop-id <id>`) |

appOFholding's menu entries are `appofholding-<id>.desktop` (`paths::desktop_id`), so both
actions match `appofholding-*.desktop` and nothing else. Its AppImages already show in the
launcher through those entries; it adds no launcher results of its own.

Everything runs through declared commands, never a shell. The JSON they read:

- `appofholding list --json`: `{version: 1, updates, apps: [{id, name, version, desktop_id,
  appimage, has_source, source, policy, status, latest, skipped, error, last_checked, update}]}`.
  It reads the database only (the last check's result), never the network. `updates` counts the
  apps with an update that is neither skipped nor pinned.
- `appofholding check --json`: asks every source now, saves the results, and prints
  `{version: 1, updates, apps: [{id, status, latest, problem, error}]}`. A source that fails is a
  row with `error`; the command still exits 0.
- `appofholding schedule status --json`: `{version: 1, installed, interval}`.

## Testing

- `uqctl integrations check uq/` (from uQ 0.4): clean apart from the programs it can't find
  where appOFholding and uQ aren't installed.
- `tests/uq/tst_widget.qml` and `tests/uq/tst_page.qml` load the widget and the page with a fake
  `ctx` and check what they show and which commands they run (CI runs them):

  ```
  QT_QPA_PLATFORM=offscreen qmltestrunner -input tests/uq/tst_widget.qml
  QT_QPA_PLATFORM=offscreen qmltestrunner -import tests/uq/imports -input tests/uq/tst_page.qml
  ```

  `tests/uq/imports/` holds a stand-in for uQ Settings' `UqLabel`.
- `cargo test` covers the JSON output, `--desktop-id`, and the GUI's `--app`, `--desktop-id` and
  `--tab` options.
