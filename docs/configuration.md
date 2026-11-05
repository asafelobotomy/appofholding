# Configuration files

`appofholding export -o setup.toml` writes everything you have set up (update sources and
policies, launch profiles, sandbox settings, hooks) to a portable TOML file. It contains no
machine-specific paths.

`appofholding apply setup.toml` makes a library match such a file. It is idempotent: applying the
same file twice changes nothing the second time. Apps that are not installed are downloaded when
the entry has a `source`, or added from a local `file`; apps not mentioned in the file are never
touched. `--dry-run` shows what would happen, `--no-install` skips downloads, and `--prune`
also removes extra profiles, hooks and sandboxes the file does not list.

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
| pacman package | downloads the release's `.pkg.tar.zst`, verifies its SHA-256, and shows `run0 pacman -U <file>`; add `--install` to run it |
| AppImage | downloads and verifies the new AppImage and replaces the running file, keeping `*.AppImage.previous` |
| `make install PREFIX=~/.local` or unknown | changes nothing and tells you where to get the release |

The package manager's own files are never overwritten by appOFholding.

## Interface settings

The only setting the graphical app keeps for itself is the colour theme: the file
`gui-theme` in the appOFholding config folder (`~/.config/appofholding/gui-theme`) holds
`system`, `gruvbox-dark` or `gruvbox-light`. Change it under Settings > Appearance; it applies the
next time the app starts. Deleting the file goes back to following the desktop. It is not part of
`export` / `apply`, because it belongs to the machine rather than to the library.
