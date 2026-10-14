//! Unsaved words, kept safe. The capture card and the editor copy what is typed to
//! Documents\Dreamy as it changes, and remove that copy after a successful save or
//! an explicit discard. After a restart (including one for an update) Dreamy restores
//! whatever was not saved yet.
//!
//! While words are unsaved, the window sets a flag. An update waits for the flag to
//! clear before the installer closes Dreamy, so typing is never cut off by an update.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::{dreamy_root, replace_file, temp_path_for};

/// The only two drafts: the capture card and the open editor.
const KINDS: [&str; 2] = ["capture", "editor"];

/// True while the window holds unsaved words. The updater waits while this is set.
#[derive(Default)]
pub struct Unsaved(AtomicBool);

impl Unsaved {
    pub fn busy(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    pub fn set(&self, busy: bool) {
        self.0.store(busy, Ordering::SeqCst);
    }
}

/// Blocks until no unsaved words remain, checking every `step`.
pub fn wait_until_clear(unsaved: &Unsaved, step: Duration) {
    while unsaved.busy() {
        std::thread::sleep(step);
    }
}

/// The file for `kind`. Any other name is refused, so a draft can never reach
/// outside the data folder or overwrite another file.
fn draft_path(dir: &Path, kind: &str) -> Result<PathBuf, String> {
    if !KINDS.contains(&kind) {
        return Err(format!("Unknown draft: {kind}"));
    }
    Ok(dir.join(format!("draft-{kind}.json")))
}

/// Save a draft in one atomic step. A newer save fully replaces the older one.
fn save_draft_in(dir: &Path, kind: &str, content: &str) -> Result<(), String> {
    let target = draft_path(dir, kind)?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    replace_file(&target, content, &temp_path_for(&target))
}

/// The saved draft, or `None` when there is none.
fn load_draft_in(dir: &Path, kind: &str) -> Result<Option<String>, String> {
    match fs::read_to_string(draft_path(dir, kind)?) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// True when a draft is still on disk. Words from before a restart count as unsaved
/// until the window reports them saved or discarded.
fn any_draft_in(dir: &Path) -> bool {
    KINDS.iter().any(|kind| draft_path(dir, kind).is_ok_and(|p| p.exists()))
}

/// Remove a draft. Removing one that is already gone is not an error.
fn clear_draft_in(dir: &Path, kind: &str) -> Result<(), String> {
    match fs::remove_file(draft_path(dir, kind)?) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Set at startup, before the update check begins, so words from before a restart already hold an update.
pub fn restore_unsaved_state(unsaved: &Unsaved) -> Result<(), String> {
    unsaved.set(any_draft_in(&dreamy_root()?));
    Ok(())
}

#[tauri::command]
pub fn save_draft(kind: String, content: String) -> Result<(), String> {
    save_draft_in(&dreamy_root()?, &kind, &content)
}

#[tauri::command]
pub fn load_draft(kind: String) -> Result<Option<String>, String> {
    load_draft_in(&dreamy_root()?, &kind)
}

#[tauri::command]
pub fn clear_draft(kind: String) -> Result<(), String> {
    clear_draft_in(&dreamy_root()?, &kind)
}

/// The window says whether it holds unsaved words right now.
#[tauri::command]
pub fn set_unsaved_changes(unsaved: tauri::State<Unsaved>, busy: bool) {
    unsaved.set(busy);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// A scratch folder for one test. Never the owner's journal.
    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dreamy-drafts-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_draft_is_kept_restored_and_cleared() {
        let dir = scratch("roundtrip");
        assert_eq!(load_draft_in(&dir, "capture").unwrap(), None);

        let text = r#"{"text":"Flying over the sea, très bien\n","lucidity":2}"#;
        save_draft_in(&dir, "capture", text).unwrap();
        assert_eq!(load_draft_in(&dir, "capture").unwrap().as_deref(), Some(text));

        clear_draft_in(&dir, "capture").unwrap();
        assert_eq!(load_draft_in(&dir, "capture").unwrap(), None);
        clear_draft_in(&dir, "capture").unwrap(); // clearing again is not an error
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_newer_save_replaces_the_older_one_and_leaves_no_temp_file() {
        let dir = scratch("replace");
        save_draft_in(&dir, "editor", "first").unwrap();
        save_draft_in(&dir, "editor", "second").unwrap();
        assert_eq!(load_draft_in(&dir, "editor").unwrap().as_deref(), Some("second"));

        let leftovers = fs::read_dir(&dir)
            .unwrap()
            .filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(leftovers, 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_the_two_known_drafts_are_accepted() {
        let dir = scratch("kinds");
        assert!(save_draft_in(&dir, "../escape", "x").is_err());
        assert!(save_draft_in(&dir, "tags", "x").is_err());
        assert!(load_draft_in(&dir, "../escape").is_err());
        assert!(clear_draft_in(&dir, "tags").is_err());
        assert!(!dir.join("draft-tags.json").exists());

        // The capture card and the editor keep separate drafts
        save_draft_in(&dir, "capture", "card").unwrap();
        save_draft_in(&dir, "editor", "editing").unwrap();
        assert_eq!(load_draft_in(&dir, "capture").unwrap().as_deref(), Some("card"));
        assert_eq!(load_draft_in(&dir, "editor").unwrap().as_deref(), Some("editing"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_draft_on_disk_counts_as_unsaved_until_it_is_cleared() {
        let dir = scratch("on-disk");
        assert!(!any_draft_in(&dir));
        save_draft_in(&dir, "editor", "words").unwrap();
        assert!(any_draft_in(&dir), "a draft left from before a restart must hold an update");
        clear_draft_in(&dir, "editor").unwrap();
        assert!(!any_draft_in(&dir));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_update_wait_holds_until_the_unsaved_flag_clears() {
        let unsaved = Unsaved::default();
        assert!(!unsaved.busy());
        unsaved.set(true);
        assert!(unsaved.busy());

        let started = Instant::now();
        std::thread::scope(|s| {
            s.spawn(|| {
                std::thread::sleep(Duration::from_millis(120));
                unsaved.set(false);
            });
            wait_until_clear(&unsaved, Duration::from_millis(5));
        });
        assert!(!unsaved.busy());
        assert!(
            started.elapsed() >= Duration::from_millis(100),
            "the wait returned before the unsaved words were cleared"
        );
    }
}
