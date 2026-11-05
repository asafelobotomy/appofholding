# appOFholding

appOFholding is an AppImage launcher/manager (a Gear Lever alternative): desktop integration, update-source
tracking, auto-update, sandboxing and launch flags. See the project plan for the roadmap.

Status: **Milestone 4 (launch model)**. M4: per-app launch profiles (args, env, prefix, working dir,
extract-and-run, opt-in elevation, built-in presets like electron-wayland / x11 / no-sandbox), extra profiles
in the right-click menu, a live command preview, and install/update hooks. M3 (update engine): M3: resumable verified downloads, atomic updates with rollback, per-app
update policy (auto / notify / manual / pinned), skip-version, running-app handling, ETag caching and a
systemd timer. M2 (sources): M1: read AppImages without executing them, integrate them into
the desktop menu, track them in a SQLite library. M2: update sources (GitHub, GitLab, Forgejo/Gitea,
zsync, plain URLs), install straight from a pasted link, update checks with explicit statuses.

```
appofholding add <file|link> [--asset PATTERN] [--channel latest|latest-pre|latest-all|TAG] [--move] [--id ID]
appofholding source show|set|clear|test ...   # `test` previews the release and which file would be chosen
appofholding check [id]
appofholding update [id] [--force] [--defer-if-running] [--keep N]
appofholding versions <id> | rollback <id> [--to VERSION]
appofholding policy <id> auto|notify|manual|pinned | skip <id> [--clear]
appofholding run [--profile P] [--print] <id> [args...]
appofholding launch presets | show <id> | set <id> [--profile P] [--preset X] [--env K=V] [--args '..'] [--prefix '..'] ...
appofholding launch hook set|show|clear|run <id> post-install|pre-update|post-update
appofholding schedule install [--interval hourly|daily|weekly] | remove | status
appofholding list
appofholding info <file|id>
appofholding remove <id> [--keep-file]
appofholding run <id> [args...]     # what generated menu entries call
```

Set `APPOFHOLDING_ROOT=/some/dir` to run entirely inside a throwaway directory.

```
cargo test --workspace
```
