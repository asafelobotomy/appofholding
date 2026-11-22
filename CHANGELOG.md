# Changelog

## 0.12.4

- **Problems are reported to uQ.** A crash in appOFholding leaves a crash record that uQ lists on Settings › Problems. When uQ's launcher runs or updates an AppImage and it fails, the failure is logged as `[APPOFHOLDING-002]` with its level, so `uqctl log` and the Problems page show it and `uqctl explain` says what to do. The codes are listed in `/usr/share/uq/diag/appofholding.toml`. This uses uQ's `uq-diag` 1.0.1, vendored in `crates/core/src/uq_diag.rs` and pinned in CI.

## 0.12.3

- **Less idle work:** the window no longer checks uQ's palette once a second while it is showing. uQ's shared uq-theme module (`crates/gui/src/uq_theme.rs`) watches the theme file and the window re-themes when it changes, shown or hidden.

## 0.12.2

- **Updates no longer break other launchers for the app.** When an update gives an AppImage a new file name (`Foo-1.0.0.AppImage` to `Foo-2.0.0.AppImage`), menu entries, URL handlers and autostart entries that the app wrote for itself now follow the new name. Before, they kept pointing at the deleted file and nothing started at login. Rolling back follows the same way.

## 0.12.1

- **Less idle work in the tray.** The check for a changed uQ theme runs only while the window is showing, and once when it is shown again, instead of every second while hidden.

## 0.12.0

- **appOFholding follows uQ's theme.** The new default colour theme, *uQ theme*, reads
  `~/.local/state/uq/theme/tokens.json` (colours and icon theme) and re-reads it while the window
  is open, so changing the theme in uQ changes appOFholding at once. Without uQ's theme it
  follows the desktop as before. *Follow the system* and the Gruvbox themes are still in
  Settings. The choice is now kept in `gui-theme-2`, so everyone starts on the uQ theme once and
  can pick another again. When uQ's theme flips between light and dark, the window restarts
  itself once nothing is running, so the icons change shade too.

## 0.11.0

- **The AppImages page in uQ Settings can now go back and add.** *Apps* has an **Add an
  AppImage from a link** row (a web link or `owner/repo`, never a path; it asks first), and each
  app's **More** menu has **Earlier versions…**, which lists the versions kept for rollback with
  a **Go back** button that shows what it would change before it does.
- `appofholding versions ID --json`, `rollback ID --dry-run --json` (the plan uQ shows),
  `rollback ID --json` and `add TARGET --json` (nothing on stdout but the answer) for programs.

## 0.10.0

- **Scan AppImages with oysterAV.** With [oysterAV](https://github.com/asafelobotomy/oysterAV)
  installed, `appofholding config set scan warn` scans each AppImage as it is added (also from a
  link or an import) or updated and keeps the answer with the app; `block` also refuses to add or
  install one in which oysterAV found a threat, and throws a flagged update download away. A
  scan on request works in every mode: `appofholding scan <id>…|--all [--json|--ndjson]` (exit 1
  on a threat), **Scan with oysterAV** in the window's app menu, and in uQ Settings → AppImages
  (Scan with oysterAV on Overview and per app, a banner and a mark on a flagged app, and an
  Antivirus choice under Preferences). `list --json` gains `scan` per app. A verdict applies to
  one build only: after an update it is gone until the new file is scanned. oysterAV only
  looks; a flagged file is never moved or deleted by appOFholding.

## 0.9.0

- **A new AppImages page in uQ Settings** (Apps → AppImages), laid out like uQ's own pages with
  three tabs. *Overview*: how many updates there are, **Check for updates** and **Update all**
  with a progress bar that follows each app, then the apps with an update and the ones that
  need attention. *Apps*: every AppImage with its icon, version, size and status, its update
  policy, Update, Skip this version, and Remove (asks first and lists everything it would
  remove, with sizes). *Preferences*: background checks, previous versions kept, the policy and
  sandbox new apps start with. Words and groups are the ones the window uses.
- `appofholding config show|set|reset` reads and changes every setting in one place (the
  update policy new sources start with, previous versions kept, the sandbox new apps start in,
  the background check schedule), checking each value before it writes. `--json` for programs;
  `defaults` and `schedule` keep working.
- `appofholding remove ID --dry-run` and `update --dry-run` show what would change without
  changing it; with `--json` they print a plan (`summary`, `changes`, `would_refuse`) that uQ
  shows before asking. `remove --json` says what was done.
- `check --ndjson` and `update --ndjson` print one JSON line per step (an app starts, its
  download percent, its result, done), for progress bars. `list --json` adds each app's
  `size_bytes`, `icon` and `external`.
- The window's remove dialog says the versions kept for rollback go too.
- Needs uQ with Settings page API 3 (uQ 0.3.0 and later); older uQ skips the page.

## 0.8.1

- Radio buttons and check boxes are easy to read in the Gruvbox themes: the selected choice
  and ticked boxes are drawn in the theme's accent colour (they were a faint grey dot and tick).
  With *Follow the system* they keep the desktop's own look.

## 0.8.0

- **Update an app from a file you already have.** Dropping (or opening) an AppImage that is a
  newer version of an app in the library asks **Update X to 2.0?** and swaps it in like a
  normal update: the file is moved into the library, the installed version is kept for
  rollback, and the update hooks run. Dropping the same or an older version says so instead of
  failing. On the command line: `appofholding update ID --from FILE [--copy]`, and `add` on
  such a file points there.
- **Importing finds newer versions too:** a loose newer copy of an installed app (for example in
  Downloads) is offered as an update of that app, selected by default; older and same-version
  copies are no longer offered as name clashes.
- **Adding a dropped AppImage moves it into the library by default.** The dialog now leads with
  **Move** and **Always move** (which stops asking); copying or leaving the file where it is are
  under *Other choices*.

## 0.7.1

- Settings tabs have short names (General, Updates, Defaults, Tray, About), so they fit when
  Settings shares the window with the app list; "Tray & startup" also showed as "Tray _…"
  because `&` marks a keyboard shortcut. The tray options no longer run past the edge of a
  narrow window.

## 0.7.0

- **Settings for every app.** Settings → *App defaults* picks the update policy new update
  sources start with (with **Apply to all apps…** to change existing ones too) and the sandbox
  new apps start in; Settings → *Updates* sets how many previous versions an update keeps (it
  was always 2; lowering it deletes the extra ones). They live in the library, so the command
  line (`appofholding defaults show|set`) and the background timer follow them too. `update
  --keep` now defaults to that setting.
- **Settings are in tabs:** General, Updates, App defaults, Tray & startup and About. The two
  kinds of update checks (the system timer and the tray's own) are together under Updates; the
  library folder can be opened from General; About offers the same update actions as the banner.
- The app page is easier to follow: the Updates tab goes status → source → policy → previous
  versions → commands (shown once there is a source), with the policy list marking the
  library's default; the Launch tab has one Save/Revert bar, an *Advanced* group, and asks before
  switching profiles with unsaved changes or deleting one; the Sandbox tab has three levels to
  pick (a changed preset is shown as *Custom*), with *Desktop integration* and *Compatibility*
  groups. The side drawer now only switches places; adding apps and checking for updates are in
  the toolbar, which also has Settings.
- Faster: the window keeps the library open instead of opening it for every call; an app page
  only redoes the expensive suggestion scan when the app, its launch settings or its sandbox
  changed, and the Launch tab only does its work while shown; an unchanged app list is not
  rebuilt; the tray's background check sends stored ETags, so unchanged releases cost nothing
  against GitHub's rate limit; zsync apps remember their SHA-1 instead of hashing the whole
  AppImage on every check; updates no longer read the new file again after downloading it, and
  delta updates verify in one read; finding running instances reads a process's environment only
  when needed; importing no longer re-reads the library per file; the database commits with
  `synchronous=NORMAL`.

## 0.6.0

- **Find an update source for AppImages that carry no update information.** appOFholding reads
  the hints inside the AppImage (electron-builder's `app-update.yml` and AppStream project links;
  when neither gives anything, a link to its own repository inside its program, as Tauri apps
  carry; then `io.github.OWNER.REPO`-style app ids from any of its desktop files, icon or window
  class) and offers **Get updates from …** as a suggestion; import results mention it too. Accepting it
  confirms that the project's newest release has an AppImage for this computer, named like the
  app or identical to the installed file, before saving the source. Applying suggestions in the
  app now runs in the background.
- An AppImage whose desktop entry has no `X-AppImage-Version` now shows the newest release
  listed in its AppStream metadata as its version.
- An update check now recognises an imported app whose file is byte for byte the newest
  release's (same SHA-256) as up to date, and remembers that release as installed. Before, such
  an app without a version showed as *unknown*.
- **Drag and drop AppImages onto the window**, on any page. appOFholding asks where each one
  should live: **move it** into the apps folder (the default), **copy it** there, or **leave it
  where it is** (it then stays your file). Tick *Do this every time* to stop asking, or pick the
  answer under Settings → *Adding AppImages*. The same choice applies to files opened with
  appOFholding from the file manager and to *Add from a file…*; files already in the apps folder
  are simply added. Previously a drop only worked on the app list and always copied.
- **uQ desktop integration** (`uq/`, installed by `make install` and every package; inert
  without uQ): a quick-settings tile with the number of updates, an **AppImages** page in uQ
  Settings (the library, Check now, Update all, each app's update policy, the background check
  schedule), and **Check for update** and **Launch options…** in the right-click menu of the
  apps appOFholding added to uQ's launcher.
- `appofholding list --json`, `check --json` and `schedule status --json` print JSON for
  scripts. `check --desktop-id appofholding-<id>.desktop` finds an app by its menu entry, and
  `check --notify` also shows the result as a desktop notification.
- `appofholding-gui --app <id>` (or `--desktop-id appofholding-<id>.desktop`) opens that app's
  page, on the tab given with `--tab overview|updates|launch|sandbox`; it also works when
  appOFholding is already running.
- **System tray.** Settings → *System tray and startup* adds a tray icon (left click shows or
  hides the window; its menu checks for updates or quits), with **minimise to the tray** and
  **close to the tray**. Wayland does not tell programs they were minimised, so there only
  closing to the tray is offered.
- **Launch at startup**, optionally straight into the tray (an XDG autostart entry that runs
  `appofholding-gui --tray`).
- **Update notifications from the tray.** While the tray icon is up, appOFholding checks every
  few hours and shows a desktop notification when an app set to *Tell me* has a new release;
  clicking it opens that app. Each version is announced once, shared with the background timer,
  so the two never both announce it.
- **One window.** Starting appOFholding while it already runs (for example from the menu while
  it sits in the tray) shows the running window, and passes it any AppImages to add.
- The app inspector was tidied:
  - *Updates*: the source form now starts filled in with the current link, file pattern and
    channel, so changing one of them no longer means retyping the link; a specific tag (such as
    `nightly`) can be followed from the window; commands to run before and after an update moved
    here from *Launch*; previous versions can be rolled back to even after the source was removed;
    a second progress bar under the one at the top was removed.
  - *Launch*: the profile picker has its own card instead of sitting inside *Quick fixes*; Save
    and Revert cover the quick fixes too; the command preview can be copied and no longer needs a
    Refresh button.
  - *Sandbox*: with protection off, the greyed-out permission cards are hidden; the rest is
    grouped into *What the app may use* and *Files* (home folder and shared folders).
  - Unsaved launch or sandbox changes are shown in a bar at the bottom of the page, whichever tab
    is open, instead of at the top of one tab.
  - *Overview*: the Launch summary counts every option (working folder, unpacking, root) and says
    when the app runs as root; the Sandbox summary says what is allowed; the File card shows the
    app's id for the command line, and "Show in folder" works for folders with unusual names.
  - The duplicate *Open* action in the page header was removed (it is the button by the name).
- Settings showed "Every day" for the background checks whatever interval was installed; it now
  reads the interval back from the timer, and `appofholding schedule status` prints it too.
- Cleanup: the timer is turned on and off by one shared function instead of copies in the CLI
  and the interface; the unused `percent-encoding` dependency and `notify::Silent` were removed;
  the interface's settings files share one loader; `docs/configuration.md` lists every interface
  setting, including the tray and autostart ones.
- The interface now creates a Qt Widgets application, so the tray icon also works on panels that
  only offer the older XEmbed tray. Packages gain a dependency on Qt Labs Platform (part of
  `qt6-declarative` on Arch and Fedora; `qml6-module-qt-labs-platform` on Debian/Ubuntu).

### Security

- **A sandboxed app could switch its own sandbox off.** The menu entry passed the AppImage's own
  `Exec=` arguments straight after `appofholding run <id>`, where `--unsandboxed` or
  `--profile` were still read as options. They now follow `--`, and menu entries made by
  earlier versions are rewritten the first time the library is opened.
- Unpacking an AppImage (for sandboxed or unpacked launches) no longer writes through, or over,
  a symlink, so a crafted image cannot place files outside its folder.
- Shared folders for a sandbox reject `..` and every spelling of, or symlink into, `/`, `/proc`,
  `/sys`, `/dev` and `/run`.
- `apply --allow-commands` is now also needed when a file sets launch arguments or environment
  variables, turns the sandbox off, gives full bus access or a writable home, shares folders, or
  (with `--prune`) removes a sandbox.
- Repeated `[Desktop Entry]` groups or `Exec=` lines in an AppImage's desktop file are merged, so
  every launch line goes through appOFholding.

### Fixed

- Pinned apps and skipped versions no longer count as updates anywhere: the app cards, the
  banner, *Update all*, the tray, `check --json` (whose rows gain `update`) and `check --notify`
  now agree with `list --json` and uQ.
- Plain download links with a query string, or no file name in the URL, can update; the file
  name comes from the server. A server that gives no ETag, Last-Modified or size shows
  *unknown* with the reason instead of an update that never goes away.
- An automatic update that keeps failing is announced once, as "update failed", instead of
  "update check failed" on every run.
- zsync delta updates are verified against the release's size and checksum (or the control
  file's SHA-1); a mismatch falls back to a full download.
- The version shown after an update prefers the desktop file, then the release tag, over stale
  AppStream metadata; AppStream releases written over several lines are read.
- Suggested sources no longer match a release just because its file name contains a word like
  "app" or "linux".
- `appofholding source test` gives the same answer as `check` for an app in the library.
- `appofholding check` exits non-zero when a check fails (`--json` still exits 0). `add FILE`
  refuses `--asset`/`--channel`, which only apply to links. `launch set` no longer overwrites a
  profile it could not read, and a corrupt saved profile is reported instead of emptied.
- Config entries need an id written as a slug (`scale-tail`, not `Scale Tail`); before, such an
  entry was half applied and failed on every later run.
- Two copies of an app no longer break each other's D-Bus connection, and two first launches of
  a sandboxed app no longer race to unpack it.
- Tray icons work for sandboxed apps on the filtered bus.
- A hook that writes a lot of errors no longer hangs until it times out.
- Removing an app also deletes its kept previous versions.
- The window no longer freezes while an app's Overview loads its suggestions or while the
  background timer is switched on or off, and the switch shows the real state after a failed
  change. Testing a source no longer marks the app as busy updating.
- Two copies of the interface starting at once after a crash no longer orphan the single-window
  socket, and a failed restart after a self-update keeps the window reachable.
- Corrected texts: when a policy installs updates (only the background timer does), the
  sandbox's home folder, and the tray message after a manual check.
- Packaging and docs: the minimum Rust version is 1.90 (required by `backhand`); the rpm and deb
  packages depend on Qt SVG; README, CONTRIBUTING and AGENTS list the real build and test
  commands.

## 0.5.0

- Suggestions: the Overview tab (and `appofholding suggest`) now points out simple improvements
  for an app on this computer, each one click to apply and easy to undo or dismiss:
  - **Let it use Wayland**: Electron apps on a Wayland session get the `electron-auto` preset,
    so older Electron versions stop running blurry through XWayland.
  - **Turn Chromium's own sandbox back on**: Electron apps whose menu entry carries
    `--no-sandbox` can drop it where the system allows user namespaces (checked, including
    Ubuntu's AppArmor restriction). Turning it off again is one click.
  - **Start it without FUSE 2**: AppImages built with the old runtime that needs `libfuse.so.2`
    are unpacked instead of mounted when that library is missing.
  - **Free up space**: offered when old versions kept for rollback take more than 200 MB.

### Fixed
- Icons larger than 512 pixels (or of an odd size) were filed in a folder icon themes never
  search, such as `hicolor/2816x2816`, so the app had no icon in the menu. They now go in the
  largest standard size folder that fits.
- Menu entries are renamed `appofholding-<id>.desktop`, which broke the link between a window and
  its entry for apps that do not declare `StartupWMClass` (taskbar showed a generic icon, pinning
  did not stick). The original entry name is now written as `StartupWMClass` when the app gives
  none.
- An update could be refused as "already being updated" for a moment after another update
  finished; the lock is now given two seconds to come free.

## 0.4.2

- Update appOFholding itself from its own window. When a new release is out, **Update now** (in
  the banner, or in Settings) says what will happen, downloads the release and checks it against
  its published SHA-256, then installs it: pacman packages through `run0 pacman -U` (your desktop
  asks for the password), AppImages in place. Afterwards it offers to restart into the new
  version. Before, a pacman install had to be updated from a terminal.

### Fixed
- Package files from earlier self-updates no longer pile up in `~/.cache/appofholding/self`; the
  command line also removes a package once it is installed.
- Dismissing the password dialog is reported as such, not as run0's "Access denied".

## 0.4.1

### Fixed
- The 0.4.0 package was built against Qt 6.12 and crashed at start on systems with an older Qt
  (Manjaro: "undefined symbol ... Qt_6_PRIVATE_API"). The interface's QML is now compiled to
  bytecode only, without Qt 6.12's lookup validation, so it uses no private Qt API, so one package works across Qt versions. CI and
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
