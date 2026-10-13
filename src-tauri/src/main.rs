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
fn place_new_dream(temp: &Path, dir: &Path, stem: &str) -> Result<String, String> {
    place_new_dream_with(temp, dir, stem, |from, to| fs::hard_link(from, to))
}

/// `place_new_dream`, with the way of linking passed in. Production passes
/// `fs::hard_link`. Tests pass a link that always fails, which forces the
/// fallback below to run.
fn place_new_dream_with(
    temp: &Path,
    dir: &Path,
    stem: &str,
    link: impl Fn(&Path, &Path) -> std::io::Result<()>,
) -> Result<String, String> {
    for n in 1..=999 {
        let name = if n == 1 {
            format!("{stem}.md")
        } else {
            format!("{stem}_{n}.md")
        };
        let target = dir.join(&name);
        match link(temp, &target) {
            // A hard link is created only if the name is free, in one step
            Ok(()) => return Ok(name),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            // Fallback, for folders that cannot make hard links (some network
            // and synced drives). Check the name is free, then rename the
            // temporary file onto it. The save path is serial, so nothing else
            // in this app can take the name in between.
            Err(_) => {
                if target.exists() {
                    continue;
                }
                fs::rename(temp, &target).map_err(|e| e.to_string())?;
                return Ok(name);
            }
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

    /// The no-hard-link fallback, forced on purpose. Every link attempt fails, so
    /// the file must be renamed into place, and the collision rules must still hold.
    #[test]
    fn dream_fallback_without_hard_links_is_collision_safe() {
        let root = scratch("fallback");
        let dir = root.join("Dreams");
        fs::create_dir_all(&dir).unwrap();
        let stamp = "2026-10-13_07-12";
        let no_links = |_: &Path, _: &Path| -> io::Result<()> {
            Err(io::Error::new(ErrorKind::Other, "hard links not supported here"))
        };

        // First dream: the temporary file is moved into place, not copied
        let temp = temp_path_for(&dir.join(format!("{stamp}.md")));
        write_synced(&temp, "one").unwrap();
        assert_eq!(
            place_new_dream_with(&temp, &dir, stamp, no_links).unwrap(),
            "2026-10-13_07-12.md"
        );
        assert!(!temp.exists());
        assert_eq!(fs::read_to_string(dir.join("2026-10-13_07-12.md")).unwrap(), "one");

        // Same minute: the fallback must not replace the first dream
        let temp = temp_path_for(&dir.join(format!("{stamp}.md")));
        write_synced(&temp, "two").unwrap();
        assert_eq!(
            place_new_dream_with(&temp, &dir, stamp, no_links).unwrap(),
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
            place_new_dream_with(&temp, &dir, stamp, first_taken).unwrap(),
            "2026-10-13_07-12_3.md"
        );
        assert_eq!(fs::read_to_string(dir.join("2026-10-13_07-12_3.md")).unwrap(), "three");

        let _ = fs::remove_dir_all(&root);
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
