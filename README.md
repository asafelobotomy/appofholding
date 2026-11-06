# appOFholding

Keep your AppImages tidy, up to date and sandboxed.

appOFholding adds AppImages to your application menu, keeps them up to date from GitHub, GitLab,
Forgejo or plain download links, lets you change how each one is launched, and can run them inside
a [bubblewrap](https://github.com/containers/bubblewrap) sandbox with limited access to your
files, network and devices. It comes as a Qt/Kirigami app (`appofholding-gui`) and a command line
tool (`appofholding`) that share one library.

## Highlights

- **Paste a link, get an app.** `appofholding add https://github.com/owner/repo` picks the right
  file for your CPU, downloads it, verifies its checksum, and adds it to the menu.
- **Bring in what you already have.** `appofholding import` finds loose AppImages, ones other tools
  (Gear Lever, AppImageLauncher) integrated and ones apps keep themselves, shows a dry-run plan,
  and asks before changing anything ([docs/importing.md](docs/importing.md)).
- **Updates you can trust.** Explicit status for every check (never a silent "checking…"), checksum
  and size verification, resumable downloads, rollback to previous versions, per-app policy
  (automatic / notify / manual / pinned), skip-this-version, and a background timer.
- **Your launch options survive updates.** Arguments, environment, command prefix, presets such
  as `electron-wayland` or `no-sandbox`, extra profiles in the right-click menu, and hooks — all
  stored in a database, never in the generated `.desktop` file.
- **Suggestions that fit your system.** Electron apps blurry under XWayland, Chromium's sandbox
  switched off without need, old runtimes that need FUSE 2, old versions eating space: each app's
  Overview (or `appofholding suggest`) offers a one-click fix you can undo.
- **Optional sandbox.** `standard` or `strict`, a private home folder per app, filtered desktop
  services, no FUSE needed. See [docs/sandbox.md](docs/sandbox.md) for what it does and its limits.
- **Repeatable setups.** `appofholding export` / `apply` move your whole setup between machines
  ([docs/configuration.md](docs/configuration.md)).

## Install

Build from source (needs Rust 1.89+, Qt 6 with QtQuick, Kirigami, CMake and a C++ compiler):

```bash
make
sudo make install PREFIX=/usr
```

On Arch Linux: `cd packaging/arch && makepkg -si`, or install the package attached to a
[release](https://github.com/asafelobotomy/appofholding/releases) with `run0 pacman -U <file>`.
After that, appOFholding updates itself: use **Update now** when it tells you a new release is out
(or `appofholding self update --install` in a terminal). It downloads and verifies the release,
then installs it with `run0 pacman -U`, so your desktop asks for your password.

Releases are made by pushing a version tag (`git tag -a v0.4.0 -m … && git push origin v0.4.0`);
the release workflow checks that every version string matches the tag, builds and tests the
Arch package, and publishes it with a source archive and `SHA256SUMS`.

Runtime needs: `bubblewrap` and `xdg-dbus-proxy` for the sandbox; `zsync` (optional) for delta
updates; `libnotify` (optional) for update notifications.

The app follows your desktop's colours. Settings > Appearance also offers **Gruvbox light** and
**Gruvbox dark** (applied on the next start).

## Quick start

```bash
appofholding add ~/Downloads/Foo.AppImage                # from a file
appofholding add https://github.com/owner/repo           # from a link
appofholding import --dry-run                            # find AppImages you already have
appofholding list
appofholding check                                       # look for updates
appofholding update                                      # install them
appofholding policy foo auto                             # update automatically from now on
appofholding schedule install --interval daily           # background checks (systemd user timer)

appofholding launch set foo --preset electron-auto       # launch options
appofholding sandbox set foo --level standard            # sandbox it
appofholding run --print foo                             # see exactly what will run

appofholding self check                                  # is there a newer appOFholding?
appofholding self update                                 # update it (see docs for how each install type works)

appofholding export -o setup.toml                        # save your setup
appofholding apply setup.toml                            # recreate it elsewhere
```

Every command has `--help`, and there are man pages (`man appofholding`) and shell completions
(`appofholding completions bash|zsh|fish`).

Set `APPOFHOLDING_ROOT=/some/folder` to run everything inside a throwaway directory.

## Documentation

- [Making your AppImage updatable](docs/embedding-update-info.md) (for AppImage authors)
- [Importing existing AppImages](docs/importing.md)
- [Sandboxing](docs/sandbox.md)
- [Configuration files](docs/configuration.md)
- [Changelog](CHANGELOG.md)

## Where things live

| What | Where |
|---|---|
| AppImages | `~/Applications` |
| Menu entries and icons | `~/.local/share/applications`, `~/.local/share/icons` |
| Database, kept versions, private homes | `~/.local/share/appofholding` |
| Unpacked copies for sandboxed apps | `~/.cache/appofholding/extracted` |
| Background timer | `~/.config/systemd/user/appofholding-update.{service,timer}` |

## Development

```bash
cargo test --workspace      # the GUI crate needs the Qt toolchain
cargo run -p appofholding-gui
```

The workspace has three crates: `appofholding-core` (everything that matters, no UI),
`appofholding` (CLI) and `appofholding-gui` (Qt/Kirigami through cxx-qt; its logic lives in a
Qt-free `service` module that is unit-tested).

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
