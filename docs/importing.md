# Importing existing AppImages

`appofholding import` (or **Import existing AppImages…** in the app) finds AppImages that are
already on your computer and brings them into the library. It always shows the whole plan first
and changes nothing until you confirm.

```bash
appofholding import --dry-run        # show the plan and stop
appofholding import                  # show the plan, then ask
appofholding import --yes            # no questions (for scripts)
```

## Where it looks

`~/Applications`, `~/AppImages`, `~/Downloads`, `~/Desktop`, `~/bin`, `~/.local/bin`, `~/opt`,
`~/Programs` and `/opt` (two folder levels deep; hidden folders are skipped), plus any `--dir`
you add (`--only-dirs` limits it to those). It also reads your menu entries and follows the ones
that launch an AppImage, so apps that keep their AppImage somewhere unusual are found too.

## Which AppImages are eligible, and who owns them

| Owner | Meaning | What import does by default |
|---|---|---|
| **Loose file** | nothing refers to it | leaves it where it is; files in Downloads/Desktop are moved into the library folder |
| **Managed by a tool** | Gear Lever or AppImageLauncher made a menu entry for it | leaves the file; the tool's menu entry stays (so the app appears twice) unless you choose `--replace-entries` |
| **Has its own menu entry** | an entry no known tool made | leaves the file and the entry |
| **Kept by the app itself** | the app stores and updates its own AppImage outside the usual folders | never moved or copied; automatic updates from appOFholding are turned off for it |

Not offered: files that are not AppImages, hidden folders, anything already in the library
(including byte-identical copies), and appOFholding's own files.

Name clashes (two AppImages called the same, or an app already in the library) are shown as
**needs a decision**: they are not selected by default, and `--include-conflicts` (or ticking
them in the app) imports them under a different id such as `foo-2`.

## Options

| Option | Effect |
|---|---|
| `--mode in-place\|copy\|move` | how loose and tool-integrated files come in (apps' own files always stay) |
| `--replace-entries` | back up the menu entries of known tools (Gear Lever, AppImageLauncher) so each app appears once |
| `--only a,b` / `--exclude a,b` | choose by name, id or the number shown in the plan |
| `--dir PATH`, `--only-dirs` | search more places, or only these |

Menu entries are **never deleted**: with `--replace-entries` they are moved to
`~/.local/share/appofholding/replaced-entries/` and `appofholding import restore` puts them
back byte for byte (it will not overwrite a file that has appeared there since). An app's own
menu entry is never touched.

## Your files stay yours

An AppImage adopted in place is still your file. Removing the app from appOFholding keeps it;
add `--delete-file` to delete it too. Files that appOFholding copied or moved into its own
folder are deleted on removal, as before.
