# Changelog

## 0.4.1

### Fixed
- The 0.4.0 package was built against Qt 6.12 and crashed at start on systems with an older Qt
  (Manjaro: "undefined symbol ... Qt_6_PRIVATE_API"). The interface's QML is now compiled to
  bytecode only, which uses no private Qt API, so one package works across Qt versions. CI and
  the release workflow refuse a build that uses private Qt symbols again.

## 0.4.0

- Library views: switch between Grid, List and List (Compact) with the buttons above the app
  list; the choice is remembered.
- Releases: pushing a `v*` tag builds the Arch package and a source archive and publishes them
  as a GitHub release, which `appofholding self update` can install from.
- Minimum Rust version is now 1.89.

### Fixed
- Sandbox: `/lib64` is recreated with its real target, so sandboxed apps start on Fedora, Debian
  and Ubuntu (it only worked where `lib64 -> usr/lib`). Apps can no longer claim the desktop's
  D-Bus names or a whole vendor's namespace through their own `.desktop` files.
- Updates never rename over a file that isn't the app's own, and a failure part-way through an
  update puts the previous version back instead of leaving a broken entry. The GUI, the CLI and
  the timer can no longer update the same app at once.
- Versions: `2.0.0-rc1` is older than `2.0.0`, and dated builds (`nightly-2026-11-05`) compare
  correctly.
- Downloads give up only when they stall (60 s without data), not after a fixed total time.
  Interrupted downloads resume only when a checksum will verify the result.
- Sandboxed apps are recognised as running (updates wait for them, and their unpacked files are
  kept); two first launches at once no longer delete each other's files.
- Plain download links keep their query string and `www.`; self-hosted GitLab sources survive
  `export`/`apply`.
- `apply` refuses hooks, command prefixes and run-as-root profiles unless `--allow-commands` is
  given; `--dry-run` lists them.
- The background timer reports a failing check once per distinct problem, and never for apps set
  to "manual".
- When appOFholding runs as an AppImage, menu entries and the timer point at the AppImage file.
- GUI: adding or dropping files with spaces or accents in the name works; unsaved Launch edits
  survive background refreshes; buttons cannot start the same update twice; installs, imports
  and self-updates have separate progress; "Download and add" installs exactly what was looked
  up; a crashed task no longer leaves the window stuck "working".
- The "follow the system" colour theme no longer draws text, headings and message boxes
  transparent; dialogs now take the chosen theme too.

## 0.3.0

- Import: `appofholding import` / **Import existing AppImages…** scans the usual places and your
  menu entries for loose AppImages, ones other tools integrated, and ones apps keep themselves;
  shows a dry-run plan, asks for confirmation, can back up (and restore) other tools' menu
  entries, and adopts files in place.
- AppImages adopted in place are the user's files: removing the app keeps them unless
  `--delete-file` is given.

- Redesigned interface: grouped app list with status at a glance, app tiles, a tabbed detail page
  (Overview, Updates, Launch, Sandbox) built from cards, and a narrow-window layout.
- Colour themes: Gruvbox light and dark, chosen in Settings > Appearance (applies on the next
  start). "Follow the system" stays the default.
- Import dialog: a clear "nothing to import" state, select all / none, plural-correct wording and
  card rows; action buttons use the new primary style.
- Internal: lint clean (clippy) across the workspace.

## 0.2.0

- Self-updater: `appofholding self check` / `self update` look at this project's GitHub releases.
  Package installs download and verify the new package and show the `run0 pacman -U` command
  (or run it with `--install`); AppImage installs replace themselves and keep the previous file;
  other installs get instructions. The background timer announces a new release once, and the
  GUI shows a banner.
- New icon: a Bag of Holding.
- Fix: cached (HTTP 304) update checks failed against GitHub because an empty "not modified"
  reply was treated as gzip data.
- `scripts/smoke.sh`: an end-to-end test for a real desktop session.

## 0.1.0

First release.

- Read AppImages without running them; add, list and remove them with correct menu entries and icons.
- Update sources: GitHub, GitLab, Forgejo/Gitea, `.zsync` control files and plain URLs; install
  straight from a pasted link; explicit update status; checksum-verified, resumable downloads.
- Update engine: atomic updates, rollback, per-app policy, skip-version, running-app handling,
  background timer with notifications, optional zsync delta updates.
- Launch profiles, presets, extra menu actions, and install/update hooks.
- Optional bubblewrap sandbox with a private home, filtered D-Bus, and an unpacked-copy cache.
- Qt/Kirigami GUI.
- `export` / `apply` for portable setups, man pages and shell completions.
