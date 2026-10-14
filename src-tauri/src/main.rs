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

/// True only for names shaped like "2026-10-13_07-12": the date and time a dream
/// is saved under. Anything else (empty, path separators, "x.md") is refused.
fn is_dream_stamp(stem: &str) -> bool {
    let bytes = stem.as_bytes();
    bytes.len() == 16
        && bytes.iter().enumerate().all(|(i, &b)| match i {
            4 | 7 | 13 => b == b'-',
            10 => b == b'_',
            _ => b.is_ascii_digit(),
        })
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

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .invoke_handler(tauri::generate_handler![
            save_dream,
            list_dreams,
            read_dream,
            read_dreams,
            load_tags,
            save_tags
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

            TrayIconBuilder::new()
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
