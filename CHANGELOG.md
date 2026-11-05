# Changelog

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
