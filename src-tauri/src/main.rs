// Prevents an extra console window from opening on Windows in release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fs;
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

/// Save the whole tag library as JSON.
#[tauri::command]
fn save_tags(content: String) -> Result<(), String> {
    let root = dreamy_root()?;
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    fs::write(root.join("tags.json"), content).map_err(|e| e.to_string())
}

/// Save one dream as a Markdown file. Returns the full path it was saved to.
#[tauri::command]
fn save_dream(filename: String, content: String) -> Result<String, String> {
    if filename.is_empty() || filename.contains(['/', '\\']) {
        return Err("Invalid file name".to_string());
    }
    let dir = dreams_dir()?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(&filename);
    fs::write(&path, content).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
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
