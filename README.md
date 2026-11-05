# appimgr

An AppImage launcher/manager (a Gear Lever alternative): desktop integration, update-source
tracking, auto-update, sandboxing and launch flags. See the project plan for the roadmap.

Status: **Milestone 2 (sources)**. M1: read AppImages without executing them, integrate them into
the desktop menu, track them in a SQLite library. M2: update sources (GitHub, GitLab, Forgejo/Gitea,
zsync, plain URLs), install straight from a pasted link, update checks with explicit statuses.

```
appimgr add <file|link> [--asset PATTERN] [--channel latest|latest-pre|latest-all|TAG] [--move] [--id ID]
appimgr source show|set|clear|test ...   # `test` previews the release and which file would be chosen
appimgr check [id]
appimgr list
appimgr info <file|id>
appimgr remove <id> [--keep-file]
appimgr run <id> [args...]     # what generated menu entries call
```

Set `APPIMGR_ROOT=/some/dir` to run entirely inside a throwaway directory.

```
cargo test --workspace
```
