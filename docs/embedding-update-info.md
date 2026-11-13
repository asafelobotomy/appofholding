# Making your AppImage updatable

appOFholding finds out where an app gets its updates from, in this order:

1. the **update information embedded in the AppImage** (this page),
2. a source you set yourself (`appofholding source set <id> <link>`, or the GUI).

Embedding it means every user gets updates with no setup.

## Without embedded update information

When an AppImage has none, appOFholding looks inside it for hints and, if it finds one, offers
it as a suggestion on the app's Overview (`appofholding suggest <id>` on the command line). It
reads, most trustworthy first:

1. electron-builder's `resources/app-update.yml` (`provider: github`, `owner`, `repo`),
2. the `<url>` links in the AppStream metadata (`usr/share/metainfo/*.xml`): `vcs-browser`,
   then `homepage` and `bugtracker`, when they point to a GitHub, GitLab or Forgejo/Codeberg
   repository (`help` and `contribute` links often name someone else's project, so they are
   not used),
3. when neither gives anything, a link to the app's own repository inside its main program
   (the `Exec` program; Rust programs, Tauri apps among them, usually carry one). Only a
   repository named like the app counts, so links to the libraries it uses are ignored,
4. reverse-DNS app ids such as `io.github.OWNER.REPO`, `io.gitlab.OWNER.REPO` or
   `page.codeberg.OWNER.REPO`, from every desktop file's name in the image, the entry's `Icon`
   and `StartupWMClass`, and the AppStream `<id>`, in a few spellings (`_` as `-`, and the
   app's name when the id squashes it).

Nothing is saved until you accept the suggestion. appOFholding then tries each place in turn and
uses the first whose newest release has exactly one AppImage for this computer, and only if that
file is named like the app or is byte for byte the file you have (the same SHA-256); if none
qualifies, nothing changes and the error lists what it found. Embedding the information is still better:
it also names the release channel and the file pattern.

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
