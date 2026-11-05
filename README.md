# appimgr

An AppImage launcher/manager (a Gear Lever alternative): desktop integration, update-source
tracking, auto-update, sandboxing and launch flags. See the project plan for the roadmap.

Status: **Milestone 3 (update engine)**. M3: resumable verified downloads, atomic updates with rollback, per-app
update policy (auto / notify / manual / pinned), skip-version, running-app handling, ETag caching and a
systemd timer. M2 (sources): M1: read AppImages without executing them, integrate them into
the desktop menu, track them in a SQLite library. M2: update sources (GitHub, GitLab, Forgejo/Gitea,
zsync, plain URLs), install straight from a pasted link, update checks with explicit statuses.

```
appimgr add <file|link> [--asset PATTERN] [--channel latest|latest-pre|latest-all|TAG] [--move] [--id ID]
appimgr source show|set|clear|test ...   # `test` previews the release and which file would be chosen
appimgr check [id]
appimgr update [id] [--force] [--defer-if-running] [--keep N]
appimgr versions <id> | rollback <id> [--to VERSION]
appimgr policy <id> auto|notify|manual|pinned | skip <id> [--clear]
appimgr schedule install [--interval hourly|daily|weekly] | remove | status
appimgr list
appimgr info <file|id>
appimgr remove <id> [--keep-file]
appimgr run <id> [args...]     # what generated menu entries call
```

Set `APPIMGR_ROOT=/some/dir` to run entirely inside a throwaway directory.

```
cargo test --workspace
```
