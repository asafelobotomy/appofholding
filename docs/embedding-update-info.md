# Making your AppImage updatable

appOFholding finds out where an app gets its updates from, in this order:

1. the **update information embedded in the AppImage** (this page),
2. a source you set yourself (`appofholding source set <id> <link>`, or the GUI).

Embedding it means every user gets updates with no setup.

## What to embed

The AppImage specification defines an ELF section called `.upd_info` that holds a single line.
appOFholding understands these forms:

| Form | Meaning |
|---|---|
| `gh-releases-zsync\|OWNER\|REPO\|latest\|NAME-*x86_64.AppImage.zsync` | newest stable GitHub release |
| `gh-releases-zsync\|OWNER\|REPO\|latest-pre\|…` | newest pre-release |
| `gh-releases-zsync\|OWNER\|REPO\|latest-all\|…` | newest release of any kind |
| `gh-releases-zsync\|OWNER\|REPO\|TAG\|…` | a specific (possibly moving) tag such as `nightly` |
| `zsync\|https://example.com/NAME-latest.AppImage.zsync` | a `.zsync` file at a fixed address |

The last field is a glob (`*` and `?`); it picks the right file when a release has several
(for example one per CPU architecture). A trailing `.zsync` is ignored for matching.

## How to embed it

Most AppImage build tools do it for you:

- **appimagetool**: `appimagetool --updateinformation "gh-releases-zsync|OWNER|REPO|latest|NAME-*x86_64.AppImage.zsync" MyApp.AppDir`
- **linuxdeploy** with the AppImage plugin: set `LDAI_UPDATE_INFORMATION` to the same string.

Publish the matching `.zsync` file next to each release asset. appOFholding reads it to learn the
build's identity (SHA-1) without downloading the whole file.

Check the result with:

```bash
appofholding info MyApp.AppImage      # shows "update info: ..."
```

## Tips

- Name release files consistently (`NAME-VERSION-x86_64.AppImage`) so the glob stays simple.
- Do not wrap an AppImage in a `.zip` or `.tar` archive; updaters cannot see inside.
- GitHub publishes a SHA-256 digest for every release asset; appOFholding verifies downloads
  against it automatically.
