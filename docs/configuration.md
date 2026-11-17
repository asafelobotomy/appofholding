# Configuration files

`appofholding export -o setup.toml` writes everything you have set up (update sources and
policies, launch profiles, sandbox settings, hooks) to a portable TOML file. Apps are named by
their update source rather than their file location; folders you shared with a sandbox, a
profile's working folder and command prefix, and a `file` you add by hand are written as given,
so check those before moving the file to another machine.

`appofholding apply setup.toml` makes a library match such a file. It is idempotent: applying the
same file twice changes nothing the second time. Apps that are not installed are downloaded when
the entry has a `source`, or added from a local `file`; apps not mentioned in the file are never
touched. `--dry-run` shows what would happen, `--no-install` skips downloads, and `--prune`
also removes extra profiles, hooks and sandboxes the file does not list.

Entries that would make appOFholding run commands or weaken an app's sandbox are refused unless
you add `--allow-commands`, so a file from someone else cannot slip them in: hooks, a profile's
command prefix, arguments or environment variables, a profile that starts the app as root, and
sandbox settings with `level = "none"`, `bus = "full"`, `home = "rw"` or any `allow` folders
(with `--prune`, an entry without a `[app.sandbox]` section for a sandboxed app counts too,
since it turns the sandbox off). `--dry-run` lists every such entry; read them first.

App ids are written the way appOFholding makes them: lowercase letters, digits and `-`
(`scale-tail`, not `Scale Tail`). A file with any other id is rejected with the id to use.

```toml
version = 1

[[app]]
id = "scale-tail"
source = "https://github.com/owner/scale_tail"
asset_pattern = "scale_tail-*-x86_64.AppImage"
policy = "auto"                    # auto | notify | manual | pinned

[app.sandbox]
level = "standard"                 # a preset, plus only what differs from it:
network = false
allow = ["/home/me/Music"]         # ":rw" makes a folder writable

[[app.profile]]
name = "default"
presets = ["electron-auto"]
args = ["--flag"]
env = { MY_VAR = "1" }

[[app.profile]]
name = "debug"                     # shows up in the app's right-click menu
presets = ["debug"]

[app.hooks]
post-update = "notify-send updated"
```

Unknown keys are errors, so a typo never silently does nothing.

## Settings for every app

A few settings apply to the whole library rather than to one app. They are kept in the library's
database, so the window (Settings → *Defaults* and *Updates*), the command line and the
background timer all follow them:

| Setting | What it does | Default |
|---|---|---|
| policy | the update policy a newly found update source starts with; existing apps keep theirs | `notify` |
| keep-versions | how many previous versions an update keeps for rollback (0–10); lowering it deletes the extra ones at once | `2` |
| sandbox | the sandbox a newly added app starts in: `none`, `standard` or `strict` | `none` |
| scan | whether [oysterAV](https://github.com/asafelobotomy/oysterAV) scans an AppImage as it is added or updated: `off`, `warn` (keep the answer, only report a threat) or `block` (also refuse to add or install one with a threat) | `off` |

```sh
appofholding defaults show
appofholding defaults set --policy auto --all   # also give every existing app this policy
appofholding defaults set --keep-versions 3 --sandbox standard
```

An `[[app]]` entry without `policy` uses the library's default when `apply` creates its source.
These settings are not part of `export`.

`appofholding config` is the same list in one place, and adds the background check schedule:

```sh
appofholding config show                       # policy, keep_versions, sandbox, schedule
appofholding config set keep_versions 3
appofholding config set schedule weekly        # off, hourly, daily or weekly
appofholding config set policy auto --all      # also give every existing app this policy
appofholding config set scan block             # scan with oysterAV; refuse an AppImage with a threat
appofholding config reset sandbox              # back to the built-in value
```

Every value is checked before anything is written, and `--json` prints
`{"version": 1, "ok": true, "settings": {...}}`, or `{"version": 1, "ok": false, "error": "..."}`
with exit status 1.

## Scanning with oysterAV

`appofholding scan <id>…` (or `--all`) has oysterAV (`oyst-cli scan <file> --ndjson`) look at the
AppImages in the library and keeps each answer (`clean`, `threat` with the threat's name, or
`error`) with the app, so `list --json`, the window and the uQ page show it. The exit status is 1
when something was found. With the `scan` setting on `warn` or `block`, a file is scanned before
it joins the library or replaces an installed version, so `block` can stop a threat before it is
installed. An answer is about one build: after an update it is gone until the new file is
scanned. oysterAV only looks; a flagged file is never moved or deleted by appOFholding, and
oysterAV is where it is quarantined. If oysterAV is busy with another job, appOFholding asks
again for a few minutes. `APPOFHOLDING_OYST_CLI` points at another `oyst-cli` (tests, odd installs).

## Updating appOFholding itself

`appofholding self check` compares the running version with the newest GitHub release, and the
background timer announces a new release once per version. What `appofholding self update` does
depends on how you installed it:

| Install | What happens |
|---|---|
| pacman package | downloads the release's `.pkg.tar.zst`, verifies its SHA-256, and shows `run0 pacman -U <file>`; add `--install` to run it. **Update now** in the window does all of this, asking for your password through the desktop |
| AppImage | downloads and verifies the new AppImage and replaces the running file, keeping `*.AppImage.previous` (from the window too) |
| `make install PREFIX=~/.local` or unknown | changes nothing and tells you where to get the release |

The package manager's own files are never overwritten by appOFholding.

## Interface settings

The graphical app keeps a few settings of its own in `~/.config/appofholding/`:

| File | Holds | Default |
|---|---|---|
| `gui-view` | library layout: `list`, `grid` or `compact` (saved as soon as you pick one) | `list` |
| `gui-theme` | colour theme: `uq` (follows uQ's theme live, the desktop's without uQ), `system`, `gruvbox-dark` or `gruvbox-light` (a new choice applies at the next start) | `uq` |
| `gui-desktop.json` | tray icon, minimise/close to the tray, update notifications, start in the tray, what to do with a dropped AppImage (`ask`, `move`, `copy`, `in-place`) | tray off, notifications on, ask |

**Launch at startup** is the XDG autostart entry `~/.config/autostart/io.github.appofholding.desktop`;
with *start in the tray* it runs `appofholding-gui --tray`. Deleting any of these files goes back
to its default. None of them is part of `export` / `apply`, because they belong to the machine
rather than to the library.
