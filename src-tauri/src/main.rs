// Prevents an extra console window from opening on Windows in release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::PathBuf;

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

/// Save the whole tag library as JSON. Written to a temporary file first and then
/// renamed over tags.json, so a crash mid-write cannot leave a half-written library.
#[tauri::command]
fn save_tags(content: String) -> Result<(), String> {
    let root = dreamy_root()?;
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let temp = root.join("tags.json.tmp");
    fs::write(&temp, content).map_err(|e| e.to_string())?;
    fs::rename(&temp, root.join("tags.json")).map_err(|e| e.to_string())
}

/// Save one dream as a Markdown file. An existing dream is never overwritten:
/// if the name is taken, "_2", "_3" and so on are added. Returns the file name
/// it was actually saved under.
#[tauri::command]
fn save_dream(filename: String, content: String) -> Result<String, String> {
    let stem = filename
        .strip_suffix(".md")
        .filter(|stem| !stem.is_empty() && !stem.contains(['/', '\\']))
        .ok_or_else(|| "Invalid file name".to_string())?;
    let dir = dreams_dir()?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    for n in 1..=999 {
        let name = if n == 1 {
            format!("{stem}.md")
        } else {
            format!("{stem}_{n}.md")
        };
        let path = dir.join(&name);
        // create_new fails if the file already exists, so nothing is replaced
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                let written = file
                    .write_all(content.as_bytes())
                    .and_then(|()| file.sync_all());
                if let Err(e) = written {
                    // Do not leave a half-written dream behind
                    drop(file);
                    let _ = fs::remove_file(&path);
                    return Err(e.to_string());
                }
                return Ok(name);
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("Too many dreams share this minute".to_string())
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

/// Read every saved dream as (file name, text) pairs, newest first.
/// Used by the Dreams sidebar so it can filter by tag, lucidity and words.
#[tauri::command]
fn read_dreams() -> Result<Vec<(String, String)>, String> {
    let dir = dreams_dir()?;
    let names = list_dreams()?;
    Ok(names
        .into_iter()
        .filter_map(|name| {
            let text = fs::read_to_string(dir.join(&name)).ok()?;
            Some((name, text))
        })
        .collect())
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

    /// Runs every check in one test, because USERPROFILE is process-wide and
    /// the checks must not race each other. Uses a scratch folder, never the
    /// owner's real Documents\Dreamy.
    #[test]
    fn saving_never_overwrites_and_tags_are_atomic() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let scratch = std::env::temp_dir().join(format!("dreamy-test-{nanos}"));
        std::env::set_var("USERPROFILE", &scratch);

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

        for bad in ["../escape.md", r"a\b.md", "notes.txt", ".md", ""] {
            assert!(save_dream(bad.into(), "x".into()).is_err(), "accepted {bad:?}");
        }

        save_tags("[1]".into()).unwrap();
        save_tags("[2]".into()).unwrap();
        assert_eq!(load_tags().unwrap(), "[2]");
        assert!(!dreamy_root().unwrap().join("tags.json.tmp").exists());

        let _ = fs::remove_dir_all(&scratch);
    }
}
