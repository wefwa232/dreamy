# Moving Dreamy's data folder: safe migration plan

**Status:** planned, not built. Nothing in this document has changed the app yet.

## Why this exists

Dreamy writes to `C:\Users\User\Documents\Dreamy`. The folder is built from the
`USERPROFILE` environment variable, so it does not follow where Windows actually
keeps Documents. On the owner's PC, Windows reports the Documents folder as
`G:\User\Documents` (checked with PowerShell on 2026-10-13). A new install,
or a later move of the Documents folder, would therefore look in the wrong place.

The legacy folder today holds:

- `Dreams\` with the owner's saved dreams (three files on 2026-10-13)
- `tags.json` (the tag library)

Those files are the owner's data. The steps below copy them, never move them,
and leave the legacy folder alone until the owner approves a removal.

## The steps

### 1. Resolve the real Windows Documents folder

Ask Windows for the Documents known folder (the `SHGetKnownFolderPath` call for
`FOLDERID_Documents`). It respects folder redirection, OneDrive and any move the
owner has made. In Tauri this is available as the path API's document directory.
Do not build the path from `USERPROFILE`.

The new location is `<Documents>\Dreamy`, with `Dreams\` and `tags.json` inside.

### 2. Detect the legacy folder

- Legacy root: `C:\Users\User\Documents\Dreamy`.
- If the legacy root is the same path as the new location, there is nothing to migrate.
- If the legacy root does not exist, there is nothing to migrate.
- If the new location's drive is not available (for example `G:` is not mounted),
  stop. Show a clear message and keep using the legacy folder. Never create
  folders on a drive that is not there.

### 3. Copy, never move or delete

For every file in the legacy folder (`Dreams\*.md` and `tags.json`):

- Copy the file into the new location. Use the same approach as the app's
  saves: write the copy to a temporary file in the target folder, flush it to
  disk, and then rename it into its final name.
- The legacy file is never opened for writing, renamed or deleted.

### 4. Handle name conflicts without overwriting either copy

For each source file, look at the target name:

| Situation | Action |
| --- | --- |
| Target name is free | Copy the file under its own name. |
| Target exists with identical bytes (same SHA-256) | Skip. It was copied before, so the migration can be re-run safely. |
| Target exists with different bytes | Copy the legacy file under a new name such as `2026-10-13_07-12_legacy.md`. Both copies are kept and listed in the report. |
| `tags.json` exists in both places and differs | Do not merge automatically. Copy the legacy file to `tags-from-legacy.json` and keep the new `tags.json` active. The owner decides how to combine them. |

Nothing is overwritten under any of these cases.

### 5. Verify before switching

Only when every copy is finished, check each one:

- The target file exists.
- Its size and SHA-256 hash match the legacy source.
- The number of dreams in the new location is at least the number of legacy
  dreams. The app's own `read_dreams` should show them.
- `tags.json` in the new location parses as a list.

If any check fails, stop. Do not switch. Write a report that names the failing
file, and leave the app on the legacy folder.

### 6. Switch the active folder

Store the chosen folder in a small settings file, in the user's AppData folder
(`%APPDATA%\Dreamy\location.json`), not inside the dream folder. Write it the
same atomic way as dreams.

- If the settings file is missing, the app uses the legacy folder, exactly as
  it does today. A failed or interrupted migration therefore changes nothing.
- The switch is the last step. Before it, the app keeps working on the legacy
  folder. After it, the app works on the new folder.

**Rollback:** delete `location.json`, and the app returns to the untouched
legacy folder.

### 7. Leave the legacy folder untouched

The legacy folder stays exactly as it is after the switch. Removing it, or
renaming it to something like `Dreamy (old)`, needs the owner's explicit
approval, and is a separate step. Until that approval is given, nothing deletes
or moves it.

## Interruption and retry

- Each copy is atomic, so an interruption leaves either a finished copy or no
  copy at all.
- Re-running the migration skips identical copies and only does the rest.
- The switch happens last and is one small atomic write, so an interruption
  before it changes nothing for the owner.
- Run the migration once at startup, before the window accepts a save, so the
  app never writes to two folders at the same time.

## Problems to watch for

- **Cloud-synced folders.** If Documents is synced by OneDrive, files may be
  online-only placeholders. Reading them for the hash can download them, so
  the migration should warn and wait rather than guess.
- **Hard links.** The save code uses hard links where the folder supports them,
  and falls back to a rename where it does not. The migration should use the
  same fallback.
- **Dreams with names the app does not expect.** Files such as
  `2026-10-13_07-12_legacy.md` do not match the date pattern. The app already
  shows them by their file name, so they still appear in the journal.

## Tests to write before building

1. Copies a fresh legacy folder into an empty new location, and the hashes match.
2. Re-running the migration skips every identical copy and changes nothing.
3. A conflicting dream is kept under a `_legacy` name, and neither copy is overwritten.
4. A conflicting `tags.json` is kept as `tags-from-legacy.json`, and the new `tags.json` is unchanged.
5. A failed verification leaves `location.json` unwritten, and the app keeps the legacy folder.
6. An interruption partway through copying leaves no partial file under a final name.
7. A missing target drive creates no folders and reports an error.
8. Deleting `location.json` returns the app to the legacy folder.

## Decisions the owner needs to make

1. Should the new location be `G:\User\Documents\Dreamy` (the Windows Documents
   folder), or should the legacy folder simply stay where it is?
2. When `tags.json` differs between the two copies, is `tags-from-legacy.json`
   enough, or would you like the app to offer a merge?
3. After a successful migration, how long should the legacy folder be kept
   before the owner approves its removal?
