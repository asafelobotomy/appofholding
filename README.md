# appimgr

An AppImage launcher/manager (a Gear Lever alternative): desktop integration, update-source
tracking, auto-update, sandboxing and launch flags. See the project plan for the roadmap.

Status: **Milestone 1 (foundation)** — read AppImages without executing them, integrate them into
the desktop menu, track them in a SQLite library, remove them cleanly.

```
appimgr add <file> [--move] [--id ID]
appimgr list
appimgr info <file|id>
appimgr remove <id> [--keep-file]
appimgr run <id> [args...]     # what generated menu entries call
```

Set `APPIMGR_ROOT=/some/dir` to run entirely inside a throwaway directory.

```
cargo test --workspace
```
