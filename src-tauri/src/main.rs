// Prevents an extra console window from opening on Windows in release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fs::{self, File};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

mod drafts;
mod updates;

/// Dreamy's data folder: Documents\Dreamy
fn dreamy_root() -> Result<PathBuf, String> {
    let home = std::env::var("USERPROFILE").map_err(|e| e.to_string())?;
    Ok(PathBuf::from(home).join("Documents").join("Dreamy"))
}

/// Where dreams are stored: Documents\Dreamy\Dreams
fn dreams_dir() -> Result<PathBuf, String> {
    Ok(dreamy_root()?.join("Dreams"))
}

/// Load the tag library (a JSON array). Returns "[]" when no tags exist yet.
#[tauri::command]
fn load_tags() -> Result<String, String> {
    let path = dreamy_root()?.join("tags.json");
    if !path.exists() {
        return Ok("[]".to_string());
    }
    fs::read_to_string(&path).map_err(|e| e.to_string())
}

/// Create `path` with `content` and flush it to disk before returning, so a
/// finished file is never only partly on disk.
fn write_synced(path: &Path, content: &str) -> std::io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(content.as_bytes())?;
    file.sync_all()
}

/// Counts temporary files made by this process, so no two in one run match.
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A temporary name in the same folder as `target` (so the final rename or link
/// never crosses a drive). It combines the process id with a counter: unique
/// among concurrent saves, and different from any leftover from an earlier,
/// interrupted run. It starts with "." and ends with ".tmp", so the journal
/// never lists it as a dream.
fn temp_path_for(target: &Path) -> PathBuf {
    let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    target.with_file_name(format!(".{name}.{}-{n}.tmp", std::process::id()))
}

/// Replace `target` with `content` in one step. The new text is written and
/// flushed to `temp` first, then renamed over `target`. If anything fails, the
/// old `target` is left exactly as it was.
fn replace_file(target: &Path, content: &str, temp: &Path) -> Result<(), String> {
    let result = write_synced(temp, content).and_then(|()| fs::rename(temp, target));
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result.map_err(|e| e.to_string())
}

/// Save the whole tag library as JSON, atomically.
#[tauri::command]
fn save_tags(content: String) -> Result<(), String> {
    let root = dreamy_root()?;
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let target = root.join("tags.json");
    replace_file(&target, &content, &temp_path_for(&target))
}

/// True only for names shaped like "2026-10-13_07-12" that also name a real
/// moment: a real month, a day that exists in that month (leap years included),
/// an hour from 0 to 23 and a minute from 0 to 59. Year 0000 is refused.
/// Anything else (empty, path separators, "x.md", "2026-02-30_10-00") is refused.
fn is_dream_stamp(stem: &str) -> bool {
    let bytes = stem.as_bytes();
    let shape_ok = bytes.len() == 16
        && bytes.iter().enumerate().all(|(i, &b)| match i {
            4 | 7 | 13 => b == b'-',
            10 => b == b'_',
            _ => b.is_ascii_digit(),
        });
    if !shape_ok {
        return false;
    }
    // Shape is checked above, so every field is plain ASCII digits
    let field = |range: std::ops::Range<usize>| -> u32 { stem[range].parse().unwrap_or(0) };
    let (year, month, day) = (field(0..4), field(5..7), field(8..10));
    let (hour, minute) = (field(11..13), field(14..16));
    year >= 1
        && (1..=12).contains(&month)
        && (1..=days_in_month(year, month)).contains(&day)
        && hour <= 23
        && minute <= 59
}

/// Days in a month of the Gregorian calendar. `month` must be 1 to 12.
fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        2 if is_leap_year(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Gregorian leap years: every 4th year, except centuries not divisible by 400.
fn is_leap_year(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Give the finished temporary file its real name without replacing anything.
/// The name is taken only if it is still free, so an existing dream is never
/// overwritten. Tries "stem.md", then "stem_2.md", "stem_3.md" and so on.
/// Move `from` to `to` in one atomic step, and fail if `to` already exists. This
/// is Windows MoveFileExW with the replace flag left out, so the existence check
/// and the move happen inside one call. Unlike `fs::rename`, it never replaces.
#[cfg(windows)]
fn move_without_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }

    let wide = |p: &Path| -> Vec<u16> {
        p.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
    };
    let (from_w, to_w) = (wide(from), wide(to));
    // Flags 0: MOVEFILE_REPLACE_EXISTING is deliberately not set
    let ok = unsafe { MoveFileExW(from_w.as_ptr(), to_w.as_ptr(), 0) };
    if ok != 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Non-Windows builds refuse rather than replace. The app is Windows-only.
#[cfg(not(windows))]
fn move_without_replace(_from: &Path, _to: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        ErrorKind::Other,
        "collision-safe placement is only implemented on Windows",
    ))
}

/// True when the destination name is already taken.
fn is_taken(e: &std::io::Error) -> bool {
    e.kind() == ErrorKind::AlreadyExists || matches!(e.raw_os_error(), Some(80) | Some(183))
}

/// True only for the Windows error codes that mean the folder's filesystem cannot
/// make hard links: 1 (invalid function), 50 (not supported), 120 (call not
/// implemented). Disk full, permission denied, antivirus blocking and bad paths
/// are real errors and are not in this list.
fn hard_links_unsupported(e: &std::io::Error) -> bool {
    matches!(e.raw_os_error(), Some(1) | Some(50) | Some(120))
}

fn place_new_dream(temp: &Path, dir: &Path, stem: &str) -> Result<String, String> {
    place_new_dream_with(
        temp,
        dir,
        stem,
        |from, to| fs::hard_link(from, to),
        move_without_replace,
    )
}

/// `place_new_dream`, with both placement operations passed in. Production uses
/// `fs::hard_link` and `move_without_replace`. Tests substitute their own, to
/// force the fallback and to create a conflicting file at the critical moment.
fn place_new_dream_with(
    temp: &Path,
    dir: &Path,
    stem: &str,
    link: impl Fn(&Path, &Path) -> std::io::Result<()>,
    move_no_replace: impl Fn(&Path, &Path) -> std::io::Result<()>,
) -> Result<String, String> {
    for n in 1..=999 {
        let name = if n == 1 {
            format!("{stem}.md")
        } else {
            format!("{stem}_{n}.md")
        };
        let target = dir.join(&name);
        match link(temp, &target) {
            // Normal path: a hard link is created only if the name is free, in one step
            Ok(()) => return Ok(name),
            Err(e) if is_taken(&e) => continue,
            // Only a filesystem that cannot make hard links takes the fallback. The
            // no-replace move is atomic and refuses an existing name, so a dream
            // created at any moment before this call is never overwritten.
            Err(e) if hard_links_unsupported(&e) => match move_no_replace(temp, &target) {
                Ok(()) => return Ok(name),
                Err(e) if is_taken(&e) => continue,
                Err(e) => return Err(e.to_string()),
            },
            // Any other failure is returned as it is, not guessed at
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("Too many dreams share this minute".to_string())
}

/// Write one dream: the full text goes to `temp` and is flushed, then it is
/// placed under a free name in `dir`. The temporary file is always removed.
fn store_new_dream(dir: &Path, stem: &str, content: &str, temp: &Path) -> Result<String, String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let result = write_synced(temp, content)
        .map_err(|e| e.to_string())
        .and_then(|()| place_new_dream(temp, dir, stem));
    let _ = fs::remove_file(temp);
    result
}

/// Save one dream as a Markdown file, atomically and without overwriting.
/// Returns the file name it was actually saved under.
#[tauri::command]
fn save_dream(filename: String, content: String) -> Result<String, String> {
    let stem = filename
        .strip_suffix(".md")
        .filter(|stem| is_dream_stamp(stem))
        .ok_or_else(|| "Invalid file name".to_string())?;
    let dir = dreams_dir()?;
    let temp = temp_path_for(&dir.join(&filename));
    store_new_dream(&dir, stem, &content, &temp)
}

/// List saved dream file names, newest first.
#[tauri::command]
fn list_dreams() -> Result<Vec<String>, String> {
    let dir = dreams_dir()?;
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut names: Vec<String> = fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| name.ends_with(".md"))
        .collect();
    names.sort();
    names.reverse();
    Ok(names)
}

/// Read every saved dream as (file name, text) pairs, newest first, plus the
/// names of any dream files that could not be read (so the app can say so).
/// Used by the Dreams sidebar so it can filter by tag, lucidity and words.
#[tauri::command]
fn read_dreams() -> Result<(Vec<(String, String)>, Vec<String>), String> {
    let dir = dreams_dir()?;
    let mut dreams = Vec::new();
    let mut unreadable = Vec::new();
    for name in list_dreams()? {
        match fs::read_to_string(dir.join(&name)) {
            Ok(text) => dreams.push((name, text)),
            Err(_) => unreadable.push(name),
        }
    }
    Ok((dreams, unreadable))
}

/// Read one saved dream file back as text, for the reader view.
#[tauri::command]
fn read_dream(filename: String) -> Result<String, String> {
    if !filename.ends_with(".md") || filename.contains(['/', '\\']) {
        return Err("Invalid file name".to_string());
    }
    let path = dreams_dir()?.join(&filename);
    fs::read_to_string(&path).map_err(|e| e.to_string())
}

/// Where deleted dreams go: Documents\Dreamy\Deleted. Nothing is destroyed; a
/// deleted dream can be moved back into Dreams by hand.
fn deleted_dir() -> Result<PathBuf, String> {
    Ok(dreamy_root()?.join("Deleted"))
}

/// True for a dream file name: a saved stamp, then optionally "_2", "_3" and so
/// on, then ".md". Anything else is refused.
fn is_dream_file(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".md") else {
        return false;
    };
    let Some(head) = stem.get(..16) else {
        return false;
    };
    let rest = &stem[16..];
    is_dream_stamp(head)
        && (rest.is_empty()
            || (rest.len() > 1
                && rest.starts_with('_')
                && rest[1..].bytes().all(|b| b.is_ascii_digit())))
}

/// The date part of a dream file name: "2026-10-13_07-12_2.md" -> "2026-10-13_07-12".
/// Only call this on a name that `is_dream_file` has accepted.
fn date_stamp_of(name: &str) -> &str {
    &name[..16]
}

/// Move `from` into `dir` under the first free name: "stem.md", then "stem_2.md"
/// and so on. Nothing already in `dir` is ever replaced.
fn move_to_free_name(from: &Path, dir: &Path, name: &str) -> Result<String, String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let stem = name.strip_suffix(".md").unwrap_or(name);
    for n in 1..=999 {
        let candidate = if n == 1 {
            format!("{stem}.md")
        } else {
            format!("{stem}_{n}.md")
        };
        match move_without_replace(from, &dir.join(&candidate)) {
            Ok(()) => return Ok(candidate),
            Err(e) if is_taken(&e) => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("Too many deleted copies of this dream".to_string())
}

/// Move the named dreams into the Deleted folder. Returns (moved, failed) names.
fn delete_dreams_in(dreams: &Path, deleted: &Path, names: &[String]) -> (Vec<String>, Vec<String>) {
    let mut moved = Vec::new();
    let mut failed = Vec::new();
    for name in names {
        let result = if is_dream_file(name) {
            move_to_free_name(&dreams.join(name), deleted, name)
        } else {
            Err("Invalid file name".to_string())
        };
        match result {
            Ok(_) => moved.push(name.clone()),
            Err(_) => failed.push(name.clone()),
        }
    }
    (moved, failed)
}

/// Move dreams to Documents\Dreamy\Deleted. Returns (moved, failed) names.
#[tauri::command]
fn delete_dreams(filenames: Vec<String>) -> Result<(Vec<String>, Vec<String>), String> {
    Ok(delete_dreams_in(&dreams_dir()?, &deleted_dir()?, &filenames))
}

/// Save an edit to one dream. Text-only edits replace the file in place, atomically.
/// A new date gives the dream a new file name (never replacing another dream).
///
/// Returns `(saved_as, leftover)`:
/// - `saved_as` is the file name the dream is now saved under.
/// - `leftover` is `None` on a clean save. It is `Some(old_name)` when the new copy
///   was written but the old copy could not be removed (for example, another program
///   has it open). Both copies then remain in Dreams. Nothing is deleted to recover
///   from that, and the caller must say so.
///
/// An `Err` means nothing new was written, and the original is untouched.
fn update_dream_in(
    dreams: &Path,
    filename: &str,
    date_stamp: &str,
    content: &str,
) -> Result<(String, Option<String>), String> {
    if !is_dream_file(filename) {
        return Err("Invalid file name".to_string());
    }
    if !is_dream_stamp(date_stamp) {
        return Err("Invalid date".to_string());
    }
    let old_path = dreams.join(filename);
    if !old_path.is_file() {
        return Err("This dream no longer exists".to_string());
    }

    if date_stamp_of(filename) == date_stamp {
        replace_file(&old_path, content, &temp_path_for(&old_path))?;
        return Ok((filename.to_string(), None));
    }

    let temp = temp_path_for(&dreams.join(format!("{date_stamp}.md")));
    let new_name = store_new_dream(dreams, date_stamp, content, &temp)?;
    match fs::remove_file(&old_path) {
        Ok(()) => Ok((new_name, None)),
        Err(_) => Ok((new_name, Some(filename.to_string()))),
    }
}

/// Save an edit to one dream. Returns `(saved_as, leftover)`, see `update_dream_in`.
#[tauri::command]
fn update_dream(
    filename: String,
    date_stamp: String,
    content: String,
) -> Result<(String, Option<String>), String> {
    update_dream_in(&dreams_dir()?, &filename, &date_stamp, &content)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updates::UpdateState::default())
        .manage(drafts::Unsaved::default())
        .manage(drafts::Recovery::default())
        .invoke_handler(tauri::generate_handler![
            save_dream,
            list_dreams,
            read_dream,
            read_dreams,
            update_dream,
            delete_dreams,
            load_tags,
            save_tags,
            drafts::save_draft,
            drafts::load_draft,
            drafts::clear_draft,
            drafts::set_unsaved_changes,
            drafts::preserve_draft,
            drafts::take_draft_recovery,
            updates::get_update_status,
            updates::dismiss_update_status,
            updates::approve_update_install
        ])
        .setup(|app| {
            // Start with Windows (re-enabled on every launch so it stays on)
            let _ = app.autolaunch().enable();

            // Launched by Windows startup: keep the window hidden, live in the tray
            if std::env::args().any(|arg| arg == "--minimized") {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }

            // System tray icon with Open and Quit
            let open_item = MenuItem::with_id(app, "open", "Open Dreamy", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit Dreamy", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open_item, &quit_item])?;

            TrayIconBuilder::with_id(updates::TRAY_ID)
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Dreamy")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            // Words left from before a restart already hold an update, before the window has opened
            if let Err(err) = drafts::restore_unsaved_state(&app.state::<drafts::Unsaved>(), &app.state::<drafts::Recovery>()) {
                eprintln!("Could not read unsaved words: {err}");
            }

            // Look for a newer release in the background. Startup does not wait for it.
            updates::check_in_background(app.handle().clone());

            Ok(())
        })
        // Closing the window hides it to the tray instead of quitting
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Dreamy");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::collections::HashSet;
    use std::io;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// A scratch folder unique to this run and to `label`. Never the owner's folder.
    fn scratch(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "dreamy-{label}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Counts temporary files left behind in a folder.
    fn leftover_temp_files(dir: &Path) -> usize {
        fs::read_dir(dir)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter(|e| {
                        let name = e.file_name().to_string_lossy().to_string();
                        name.starts_with('.') && name.ends_with(".tmp")
                    })
                    .count()
            })
            .unwrap_or(0)
    }

    /// A link that behaves as a filesystem with no hard-link support does.
    fn unsupported_link(_: &Path, _: &Path) -> io::Result<()> {
        Err(io::Error::from_raw_os_error(50))
    }

    /// Runs the command-level checks against a scratch home folder. USERPROFILE is
    /// process-wide, so only this test changes it.
    #[test]
    fn saving_never_overwrites_and_tags_are_atomic() {
        let home = scratch("home");
        std::env::set_var("USERPROFILE", &home);

        // No Dreams folder yet: an empty journal, not an error
        let (dreams, unreadable) = read_dreams().unwrap();
        assert!(dreams.is_empty() && unreadable.is_empty());

        let stamp = "2026-10-13_07-12.md";
        assert_eq!(save_dream(stamp.into(), "first".into()).unwrap(), stamp);
        assert_eq!(
            save_dream(stamp.into(), "second".into()).unwrap(),
            "2026-10-13_07-12_2.md"
        );
        assert_eq!(
            save_dream(stamp.into(), "third".into()).unwrap(),
            "2026-10-13_07-12_3.md"
        );

        let dir = dreams_dir().unwrap();
        assert_eq!(fs::read_to_string(dir.join(stamp)).unwrap(), "first");
        assert_eq!(fs::read_to_string(dir.join("2026-10-13_07-12_2.md")).unwrap(), "second");
        assert_eq!(fs::read_to_string(dir.join("2026-10-13_07-12_3.md")).unwrap(), "third");

        for bad in [
            "../escape.md",
            r"a\b.md",
            "notes.txt",
            ".md",
            "",
            "2026-10-13 07-12.md",
            "2026-10-13_07-12x.md",
        ] {
            assert!(save_dream(bad.into(), "x".into()).is_err(), "accepted {bad:?}");
        }

        // Unicode text survives a save and a read unchanged
        let unicode = "Über ✦ 夢 — flying";
        let kept = save_dream("2026-10-13_10-00.md".into(), unicode.into()).unwrap();
        assert_eq!(fs::read_to_string(dir.join(&kept)).unwrap(), unicode);

        // A dream file that cannot be read is reported, not silently dropped.
        // A folder named like a dream cannot be read as text.
        let broken = "2026-10-13_09-00.md";
        fs::create_dir_all(dir.join(broken)).unwrap();
        let (dreams, unreadable) = read_dreams().unwrap();
        assert_eq!(dreams.len(), 4);
        assert_eq!(unreadable, vec![broken.to_string()]);
        fs::remove_dir(dir.join(broken)).unwrap();

        // Tags: each save replaces the previous library, and nothing is left behind
        save_tags("[1]".into()).unwrap();
        save_tags("[2]".into()).unwrap();
        assert_eq!(load_tags().unwrap(), "[2]");
        assert_eq!(leftover_temp_files(&dreamy_root().unwrap()), 0);

        let _ = fs::remove_dir_all(&home);
    }

    /// Interrupted and failed writes must leave existing files intact. A write is
    /// "interrupted" by putting a folder where its temporary file would go, which
    /// makes the write fail exactly where a crash or a full disk would stop it.
    #[test]
    fn interrupted_writes_leave_existing_files_intact() {
        let root = scratch("io");
        let dir = root.join("Dreams");
        fs::create_dir_all(&dir).unwrap();

        // Tags: a failed replacement keeps the old library
        let tags = root.join("tags.json");
        fs::write(&tags, "[\"kept\"]").unwrap();
        let blocked = temp_path_for(&tags);
        fs::create_dir_all(&blocked).unwrap();
        assert!(replace_file(&tags, "[\"new\"]", &blocked).is_err());
        assert_eq!(fs::read_to_string(&tags).unwrap(), "[\"kept\"]");
        fs::remove_dir(&blocked).unwrap();

        // Tags: a successful replacement goes through a temporary file in the same folder
        let temp = temp_path_for(&tags);
        assert_eq!(temp.parent(), tags.parent());
        replace_file(&tags, "[\"new\"]", &temp).unwrap();
        assert_eq!(fs::read_to_string(&tags).unwrap(), "[\"new\"]");
        assert_eq!(leftover_temp_files(&root), 0);

        // Dreams: a failed save creates no dream file at all
        let stamp = "2026-10-13_07-12";
        let blocked = temp_path_for(&dir.join(format!("{stamp}.md")));
        fs::create_dir_all(&blocked).unwrap();
        assert!(store_new_dream(&dir, stamp, "lost", &blocked).is_err());
        let dream_files = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".md"))
            .count();
        assert_eq!(dream_files, 0);
        fs::remove_dir(&blocked).unwrap();

        // Dreams: a crash that left a temporary file behind hides nothing and blocks nothing
        fs::write(dir.join(".2026-10-13_07-12.999-0.tmp"), "half").unwrap();
        let t = temp_path_for(&dir.join("2026-10-13_07-12.md"));
        assert_eq!(
            store_new_dream(&dir, stamp, "fine", &t).unwrap(),
            "2026-10-13_07-12.md"
        );
        assert_eq!(fs::read_to_string(dir.join("2026-10-13_07-12.md")).unwrap(), "fine");

        // Dreams: a retry in the same minute never replaces the first dream
        let t = temp_path_for(&dir.join("2026-10-13_07-12.md"));
        assert_eq!(
            store_new_dream(&dir, stamp, "second", &t).unwrap(),
            "2026-10-13_07-12_2.md"
        );
        assert_eq!(fs::read_to_string(dir.join("2026-10-13_07-12.md")).unwrap(), "fine");

        let _ = fs::remove_dir_all(&root);
    }

    /// The no-hard-link fallback, forced on purpose. Hard links report unsupported,
    /// so the file goes through the no-replace move, and the collision rules hold.
    #[test]
    fn dream_fallback_without_hard_links_is_collision_safe() {
        let root = scratch("fallback");
        let dir = root.join("Dreams");
        fs::create_dir_all(&dir).unwrap();
        let stamp = "2026-10-13_07-12";

        // First dream: the temporary file is moved into place, not copied
        let temp = temp_path_for(&dir.join(format!("{stamp}.md")));
        write_synced(&temp, "one").unwrap();
        assert_eq!(
            place_new_dream_with(&temp, &dir, stamp, unsupported_link, move_without_replace).unwrap(),
            "2026-10-13_07-12.md"
        );
        assert!(!temp.exists());
        assert_eq!(fs::read_to_string(dir.join("2026-10-13_07-12.md")).unwrap(), "one");

        // Same minute: the fallback must not replace the first dream
        let temp = temp_path_for(&dir.join(format!("{stamp}.md")));
        write_synced(&temp, "two").unwrap();
        assert_eq!(
            place_new_dream_with(&temp, &dir, stamp, unsupported_link, move_without_replace).unwrap(),
            "2026-10-13_07-12_2.md"
        );
        assert!(!temp.exists());
        assert_eq!(fs::read_to_string(dir.join("2026-10-13_07-12.md")).unwrap(), "one");
        assert_eq!(fs::read_to_string(dir.join("2026-10-13_07-12_2.md")).unwrap(), "two");

        // Taken names are skipped: a link that reports "already exists" on its first
        // attempt moves on to the next name, and the real link succeeds there.
        let temp = temp_path_for(&dir.join(format!("{stamp}.md")));
        write_synced(&temp, "three").unwrap();
        let attempts = Cell::new(0);
        let first_taken = |from: &Path, to: &Path| {
            attempts.set(attempts.get() + 1);
            if attempts.get() == 1 {
                Err(io::Error::from(ErrorKind::AlreadyExists))
            } else {
                fs::hard_link(from, to)
            }
        };
        assert_eq!(
            place_new_dream_with(&temp, &dir, stamp, first_taken, move_without_replace).unwrap(),
            "2026-10-13_07-12_3.md"
        );
        assert_eq!(fs::read_to_string(dir.join("2026-10-13_07-12_3.md")).unwrap(), "three");

        let _ = fs::remove_dir_all(&root);
    }

    /// The no-replace move refuses an existing destination and leaves it untouched.
    #[test]
    fn no_replace_move_refuses_an_existing_destination() {
        let root = scratch("noreplace");
        let existing = root.join("existing.md");
        let incoming = root.join("incoming.tmp");
        fs::write(&existing, "keep me").unwrap();
        fs::write(&incoming, "new text").unwrap();

        let err = move_without_replace(&incoming, &existing).unwrap_err();
        assert!(is_taken(&err), "unexpected error: {err}");
        assert_eq!(fs::read_to_string(&existing).unwrap(), "keep me");
        assert_eq!(fs::read_to_string(&incoming).unwrap(), "new text");

        // And it still moves when the destination is free
        let free = root.join("free.md");
        move_without_replace(&incoming, &free).unwrap();
        assert_eq!(fs::read_to_string(&free).unwrap(), "new text");
        assert!(!incoming.exists());

        let _ = fs::remove_dir_all(&root);
    }

    /// A dream created by another writer just before placement is never replaced.
    /// The conflicting file appears inside the placement step itself, after the
    /// hard link has reported "unsupported", which is the window a check-then-rename
    /// fallback would lose.
    #[test]
    fn fallback_cannot_replace_a_dream_created_just_before_placement() {
        let root = scratch("race");
        let dir = root.join("Dreams");
        fs::create_dir_all(&dir).unwrap();
        let stamp = "2026-10-13_07-12";

        let racer_written = Cell::new(false);
        let racing_move = |from: &Path, to: &Path| {
            let is_first_name = to.file_name().and_then(|n| n.to_str()) == Some("2026-10-13_07-12.md");
            if is_first_name && !racer_written.get() {
                // The other writer takes the name immediately before our placement
                fs::write(to, "racer").unwrap();
                racer_written.set(true);
            }
            move_without_replace(from, to)
        };

        let temp = temp_path_for(&dir.join(format!("{stamp}.md")));
        write_synced(&temp, "mine").unwrap();
        assert_eq!(
            place_new_dream_with(&temp, &dir, stamp, unsupported_link, racing_move).unwrap(),
            "2026-10-13_07-12_2.md"
        );
        assert!(racer_written.get(), "the conflict was never created");
        assert_eq!(fs::read_to_string(dir.join("2026-10-13_07-12.md")).unwrap(), "racer");
        assert_eq!(fs::read_to_string(dir.join("2026-10-13_07-12_2.md")).unwrap(), "mine");

        let _ = fs::remove_dir_all(&root);
    }

    /// Real failures are returned as they are. They must not take the fallback, and
    /// must not create a dream.
    #[test]
    fn disk_full_and_permission_errors_do_not_trigger_the_fallback() {
        let root = scratch("realerr");
        let dir = root.join("Dreams");
        fs::create_dir_all(&dir).unwrap();
        let stamp = "2026-10-13_07-12";
        // The move must never run for these errors; if it does, the test panics
        let must_not_move = |_: &Path, _: &Path| -> io::Result<()> {
            panic!("fallback ran for a real error")
        };

        for code in [112, 5] {
            let link = move |_: &Path, _: &Path| -> io::Result<()> {
                Err(io::Error::from_raw_os_error(code))
            };
            let temp = temp_path_for(&dir.join(format!("{stamp}.md")));
            write_synced(&temp, "x").unwrap();
            assert!(place_new_dream_with(&temp, &dir, stamp, link, must_not_move).is_err());
            assert!(!dir.join("2026-10-13_07-12.md").exists(), "error {code} created a dream");
            let _ = fs::remove_file(&temp);
        }

        let _ = fs::remove_dir_all(&root);
    }

    /// Error classification: only the unsupported codes take the fallback, and only
    /// the existing-name codes count as taken.
    #[test]
    fn error_classification_is_exact() {
        assert!(hard_links_unsupported(&io::Error::from_raw_os_error(50)));
        assert!(hard_links_unsupported(&io::Error::from_raw_os_error(1)));
        assert!(hard_links_unsupported(&io::Error::from_raw_os_error(120)));
        for code in [112, 5, 3, 32] {
            assert!(!hard_links_unsupported(&io::Error::from_raw_os_error(code)), "code {code}");
        }
        assert!(is_taken(&io::Error::from(ErrorKind::AlreadyExists)));
        assert!(is_taken(&io::Error::from_raw_os_error(183)));
        assert!(!is_taken(&io::Error::from_raw_os_error(112)));
    }

    /// Deleting moves dreams into Deleted with their text intact, and never replaces
    /// an earlier deleted dream of the same name. Bad names and missing dreams are
    /// reported as failures and touch nothing.
    #[test]
    fn delete_moves_dreams_aside_and_never_replaces() {
        let root = scratch("delete");
        let dir = root.join("Dreams");
        let deleted = root.join("Deleted");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("2026-10-13_07-12.md"), "one").unwrap();
        fs::write(dir.join("2026-10-13_07-12_2.md"), "two").unwrap();

        let names = vec![
            "2026-10-13_07-12.md".to_string(),
            "2026-10-13_07-12_2.md".to_string(),
            "../escape.md".to_string(),
            "2026-10-14_08-00.md".to_string(),
        ];
        let (moved, failed) = delete_dreams_in(&dir, &deleted, &names);
        assert_eq!(moved, names[..2].to_vec());
        assert_eq!(failed, names[2..].to_vec());
        assert!(!dir.join("2026-10-13_07-12.md").exists());
        assert_eq!(fs::read_to_string(deleted.join("2026-10-13_07-12.md")).unwrap(), "one");
        assert_eq!(fs::read_to_string(deleted.join("2026-10-13_07-12_2.md")).unwrap(), "two");

        // The same name deleted again is kept beside the first one, not over it
        fs::write(dir.join("2026-10-13_07-12.md"), "three").unwrap();
        let (moved, _) = delete_dreams_in(&dir, &deleted, &["2026-10-13_07-12.md".to_string()]);
        assert_eq!(moved, vec!["2026-10-13_07-12.md".to_string()]);
        assert_eq!(fs::read_to_string(deleted.join("2026-10-13_07-12.md")).unwrap(), "one");
        assert_eq!(fs::read_to_string(deleted.join("2026-10-13_07-12_3.md")).unwrap(), "three");

        let _ = fs::remove_dir_all(&root);
    }

    /// Editing keeps a dream's file when only its text changes, moves it when the
    /// date changes, never replaces another dream on the way, and refuses bad input.
    #[test]
    fn edits_keep_names_for_text_and_move_for_new_dates() {
        let root = scratch("edit");
        let dir = root.join("Dreams");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("2026-10-13_07-12.md"), "old").unwrap();
        fs::write(dir.join("2026-10-13_07-12_2.md"), "second").unwrap();

        // Text only: same file, new text, no leftover copy
        assert_eq!(
            update_dream_in(&dir, "2026-10-13_07-12.md", "2026-10-13_07-12", "new").unwrap(),
            ("2026-10-13_07-12.md".to_string(), None)
        );
        assert_eq!(fs::read_to_string(dir.join("2026-10-13_07-12.md")).unwrap(), "new");

        // A suffixed file keeps its suffix while its date is unchanged
        assert_eq!(
            update_dream_in(&dir, "2026-10-13_07-12_2.md", "2026-10-13_07-12", "second edit").unwrap().0,
            "2026-10-13_07-12_2.md"
        );
        assert_eq!(fs::read_to_string(dir.join("2026-10-13_07-12_2.md")).unwrap(), "second edit");

        // New date: the file moves, the old name is gone, and the taken name is kept
        fs::write(dir.join("2026-10-14_09-00.md"), "taken").unwrap();
        assert_eq!(
            update_dream_in(&dir, "2026-10-13_07-12.md", "2026-10-14_09-00", "moved").unwrap(),
            ("2026-10-14_09-00_2.md".to_string(), None)
        );
        assert!(!dir.join("2026-10-13_07-12.md").exists());
        assert_eq!(fs::read_to_string(dir.join("2026-10-14_09-00.md")).unwrap(), "taken");
        assert_eq!(fs::read_to_string(dir.join("2026-10-14_09-00_2.md")).unwrap(), "moved");

        // Refused: bad file names, bad dates, and dreams that do not exist
        assert!(update_dream_in(&dir, "../x.md", "2026-10-14_09-00", "x").is_err());
        assert!(update_dream_in(&dir, "2026-10-14_09-00.md", "not-a-date", "x").is_err());
        assert!(update_dream_in(&dir, "2026-10-14_09-00.md", "2026-02-30_10-00", "x").is_err());
        assert!(update_dream_in(&dir, "2026-10-15_10-00.md", "2026-10-15_10-00", "x").is_err());
        assert!(!dir.join("2026-10-15_10-00.md").exists());

        let _ = fs::remove_dir_all(&root);
    }

    /// Windows only. The old copy is held open with no sharing, so Windows cannot
    /// remove it. The new copy must still be written, and the result must say that
    /// both copies remain. Neither copy may be deleted to recover.
    #[cfg(windows)]
    #[test]
    fn date_change_reports_both_copies_when_the_old_one_cannot_be_removed() {
        use std::os::windows::fs::OpenOptionsExt;

        let root = scratch("partial");
        let dir = root.join("Dreams");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("2026-10-13_07-12.md"), "old text").unwrap();

        // share_mode(0): no other handle may delete or write this file while it is open
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(dir.join("2026-10-13_07-12.md"))
            .unwrap();
        let (saved, leftover) =
            update_dream_in(&dir, "2026-10-13_07-12.md", "2026-10-14_09-00", "new text").unwrap();
        drop(held);

        assert_eq!(saved, "2026-10-14_09-00.md");
        assert_eq!(leftover, Some("2026-10-13_07-12.md".to_string()));
        assert_eq!(fs::read_to_string(dir.join(&saved)).unwrap(), "new text");
        assert_eq!(fs::read_to_string(dir.join("2026-10-13_07-12.md")).unwrap(), "old text");

        let _ = fs::remove_dir_all(&root);
    }

    /// Calendar rules: real months, days that exist in that month (leap years
    /// included), hours 0 to 23, minutes 0 to 59. Applies to new names and edits.
    #[test]
    fn date_stamps_must_be_real_calendar_moments() {
        for valid in [
            "2026-10-13_07-12",
            "2024-02-29_12-00", // leap year
            "2000-02-29_00-00", // divisible by 400, so a leap year
            "2026-04-30_23-59",
            "2026-12-31_23-59",
            "2026-01-01_00-00",
        ] {
            assert!(is_dream_stamp(valid), "rejected valid {valid}");
        }
        for invalid in [
            "2023-02-29_12-00", // not a leap year
            "1900-02-29_12-00", // divisible by 100 but not 400, so not a leap year
            "2026-02-30_12-00",
            "2026-04-31_12-00", // April has 30 days
            "2026-13-01_12-00",
            "2026-00-10_12-00",
            "2026-10-00_12-00",
            "2026-10-32_12-00",
            "2026-10-13_24-00",
            "2026-10-13_12-60",
            "0000-01-01_12-00",
            "2026-10-13_07-12x",
        ] {
            assert!(!is_dream_stamp(invalid), "accepted invalid {invalid}");
        }

        // The same rule applies to file names on disk, so a bad name is never deleted or edited
        assert!(is_dream_file("2024-02-29_12-00_2.md"));
        assert!(!is_dream_file("2023-02-29_12-00.md"));
    }

    /// Temporary names are unique, sit in the destination's folder, and stay hidden.
    #[test]
    fn temporary_names_are_unique_and_in_the_same_folder() {
        let target = Path::new("Dreams").join("2026-10-13_07-12.md");
        let mut seen = HashSet::new();
        for _ in 0..1000 {
            let temp = temp_path_for(&target);
            assert_eq!(temp.parent(), target.parent());
            let name = temp.file_name().unwrap().to_string_lossy().to_string();
            assert!(name.starts_with('.') && name.ends_with(".tmp"));
            assert!(seen.insert(name), "temporary name reused");
        }
    }
}
