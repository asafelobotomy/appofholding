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

Entries that would make appOFholding run commands (hooks, a profile's command prefix, or a
profile that starts the app as root) are refused unless you add `--allow-commands`, so a file
from someone else cannot slip them in. `--dry-run` lists every such command; read them first.

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

The graphical app keeps two settings of its own. The library layout is the file `gui-view` in the
same folder (`list`, `grid` or `compact`); it is saved as soon as you pick a layout. The colour
theme is `gui-theme` (`system`, `gruvbox-dark` or `gruvbox-light`); change it under
Settings > Appearance and it applies the next time the app starts. Both live in
`~/.config/appofholding/`. Deleting a file goes back to its default (the desktop colours, the list
layout). Neither is part of `export` / `apply`, because they belong to the machine rather than to
the library.
