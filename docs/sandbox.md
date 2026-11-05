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
  other services are invisible. Apps may own the D-Bus names their `.desktop` files declare, as
  long as those have at least three parts and are not the desktop's own (`org.freedesktop.*`,
  `org.gnome.*`, KDE's wallet and shell, ...).
- Chromium and Electron apps cannot nest their own sandbox inside this one, so `--no-sandbox` is
  added automatically (`--chromium-workaround false` turns that off).

## Limits (read this)

This is real isolation but weaker than Flatpak:

- there is no per-app D-Bus identity and no portal document store, so file pickers return real
  paths; use `--allow` for folders an app needs;
- an app with `--home rw` or `--bus full` can do a lot of damage;
- on an X11 session (or with XWayland), any app that can draw windows can read keystrokes and
  inject input into other X11 windows: both levels keep the display. `--display false` hides
  the X11 socket file, but while the network is on the app can still reach X through its
  abstract socket, so it only really cuts X11 off together with `--network false`;
- each launch is its own sandbox, so a second launch of a "single instance" app (opening a link
  from another program, for example) cannot hand over to the one already running and may
  complain that its profile is in use;
- a kernel or bubblewrap bug breaks any sandbox.

Run `appofholding sandbox check` to see whether your machine can run sandboxes.
