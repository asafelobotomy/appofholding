# Changelog

## Unreleased

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
