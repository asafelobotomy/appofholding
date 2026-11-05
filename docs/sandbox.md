# Sandboxing

`appofholding sandbox set <id> --level standard` runs an app inside a
[bubblewrap](https://github.com/containers/bubblewrap) sandbox (the same tool Flatpak uses).
Sandboxing is **off** until you turn it on for an app.

## Levels

| | Network | Sound | Graphics | Desktop services | Your files |
|---|---|---|---|---|---|
| `standard` | yes | yes | yes | portals, notifications, tray (filtered) | none; own private home |
| `strict` | no | no | yes | none | none; own private home |

Change single options with `--network`, `--audio`, `--gpu`, `--devices` (cameras), `--display`,
`--home private|ro|rw`, `--bus off|filtered|full`, and share folders with `--allow PATH[:rw]`.
Changing a preset makes it `custom`.

`appofholding run --print <id>` shows the exact command. `appofholding run --unsandboxed <id>`
starts the app without the sandbox once, for debugging.

## How it works

- The AppImage is unpacked into a cache (by SHA-256) and run from there, so FUSE is not needed.
- The app sees a read-only system, an empty `/tmp`, an almost empty environment, its own
  process list, and only the sockets and devices its settings allow.
- Its **private home** (`~/.local/share/appofholding/apps/<id>/home`) persists between runs and
  is kept when you remove the app, unless you pass `--purge-data`.
- The filtered session bus uses `xdg-dbus-proxy`: portals, notifications and tray icons work;
  other services are invisible. Apps may own the D-Bus names their `.desktop` files declare.
- Chromium and Electron apps cannot nest their own sandbox inside this one, so `--no-sandbox` is
  added automatically (`--chromium-workaround false` turns that off).

## Limits (read this)

This is real isolation but weaker than Flatpak:

- there is no per-app D-Bus identity and no portal document store, so file pickers return real
  paths; use `--allow` for folders an app needs;
- an app with `--home rw` or `--bus full` can do a lot of damage;
- a kernel or bubblewrap bug breaks any sandbox.

Run `appofholding sandbox check` to see whether your machine can run sandboxes.
