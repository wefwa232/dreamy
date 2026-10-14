//! Unsaved words, kept safe. The capture card and the editor copy what is typed to
//! Documents\Dreamy as it changes, and remove that copy after a successful save or
//! an explicit discard. After a restart (including one for an update) Dreamy restores
//! whatever was not saved yet.
//!
//! While words are unsaved, the window sets a flag. An update is installed only after
//! the window approves it, and the approval is refused while the flag is set, so typing
//! is never cut off by an update.
//!
//! A stored copy that is damaged, or whose dream is gone, is never deleted. It is moved
//! aside under a recovery name in the same folder, and the window says where it went.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{dreamy_root, dreams_dir, is_dream_file, is_taken, move_without_replace, replace_file, temp_path_for};

/// The only two drafts: the capture card and the open editor.
const KINDS: [&str; 2] = ["capture", "editor"];

/// True while the window holds unsaved words. The updater refuses to install while this is set.
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

/// Names of drafts moved aside at startup. The window reads them once, so it can say where the words went.
#[derive(Default)]
pub struct Recovery(Mutex<Vec<String>>);

impl Recovery {
    fn set(&self, names: Vec<String>) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = names;
    }

    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(|e| e.into_inner()))
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
pub(crate) fn save_draft_in(dir: &Path, kind: &str, content: &str) -> Result<(), String> {
    let target = draft_path(dir, kind)?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    replace_file(&target, content, &temp_path_for(&target))
}

/// The saved draft, or `None` when there is none.
pub(crate) fn load_draft_in(dir: &Path, kind: &str) -> Result<Option<String>, String> {
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
pub(crate) fn clear_draft_in(dir: &Path, kind: &str) -> Result<(), String> {
    match fs::remove_file(draft_path(dir, kind)?) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Seconds since 1970. Names recovery copies, so a new copy never takes an earlier one's name.
fn recovery_stamp() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// True when a stored draft can still be restored: it is a JSON object, and an editor
/// draft names a dream that still exists.
fn draft_is_usable(text: &str, kind: &str, dreams: &Path) -> bool {
    let Ok(serde_json::Value::Object(fields)) = serde_json::from_str::<serde_json::Value>(text) else {
        return false;
    };
    if kind != "editor" {
        return true;
    }
    fields
        .get("name")
        .and_then(|name| name.as_str())
        .is_some_and(|name| is_dream_file(name) && dreams.join(name).is_file())
}

/// Moves a draft aside under a recovery name, so its words are kept but it no longer
/// counts as active. Names never overwrite: "draft-<kind>.recovered-<stamp>.json", then
/// "_2", "_3" and so on. Returns `None` when there was no draft to move.
fn preserve_draft_in(dir: &Path, kind: &str, stamp: u64) -> Result<Option<String>, String> {
    let active = draft_path(dir, kind)?;
    if !active.exists() {
        return Ok(None);
    }
    for n in 1..=999 {
        let name = if n == 1 {
            format!("draft-{kind}.recovered-{stamp}.json")
        } else {
            format!("draft-{kind}.recovered-{stamp}_{n}.json")
        };
        match move_without_replace(&active, &dir.join(&name)) {
            Ok(()) => return Ok(Some(name)),
            Err(e) if is_taken(&e) => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("Too many recovered copies of this draft".to_string())
}

/// Checks the stored drafts at startup. A draft that cannot be read, is not a JSON
/// object, or belongs to a dream that no longer exists is moved aside, never deleted.
/// Each recovery name is added to `moved`.
fn set_aside_damaged_drafts_in(dir: &Path, dreams: &Path, stamp: u64, moved: &mut Vec<String>) -> Result<(), String> {
    for kind in KINDS {
        let damaged = match load_draft_in(dir, kind) {
            Ok(None) => false,
            Ok(Some(text)) => !draft_is_usable(&text, kind, dreams),
            Err(_) => true,
        };
        if damaged {
            if let Some(name) = preserve_draft_in(dir, kind, stamp)? {
                moved.push(name);
            }
        }
    }
    Ok(())
}

/// Runs at startup, before the window opens. Damaged drafts are moved aside first, then
/// the flag is set from whatever drafts remain. The update check starts after this.
pub fn restore_unsaved_state(unsaved: &Unsaved, recovery: &Recovery) -> Result<(), String> {
    let root = dreamy_root()?;
    let mut moved = Vec::new();
    let swept = set_aside_damaged_drafts_in(&root, &dreams_dir()?, recovery_stamp(), &mut moved);
    unsaved.set(any_draft_in(&root));
    recovery.set(moved);
    swept
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

/// Moves a draft aside when its dream turns out to be gone after startup. Returns its recovery name.
#[tauri::command]
pub fn preserve_draft(kind: String) -> Result<Option<String>, String> {
    preserve_draft_in(&dreamy_root()?, &kind, recovery_stamp())
}

/// The drafts moved aside at startup, handed over once so the window can say where they went.
#[tauri::command]
pub fn take_draft_recovery(recovery: tauri::State<Recovery>) -> Vec<String> {
    recovery.take()
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// A draft that cannot be read, or is not a JSON object, is moved aside under a
    /// recovery name with its exact bytes. It stops counting as an active draft.
    #[test]
    fn a_malformed_draft_is_moved_aside_and_its_words_kept() {
        let root = scratch("malformed");
        let dreams = root.join("Dreams");
        fs::create_dir_all(&dreams).unwrap();

        for (stamp, broken) in [(1000, r#"{"text": "half a sent"#), (1001, "[1, 2]"), (1002, "\"words\"")] {
            save_draft_in(&root, "capture", broken).unwrap();
            let mut moved = Vec::new();
            set_aside_damaged_drafts_in(&root, &dreams, stamp, &mut moved).unwrap();
            assert_eq!(moved, vec![format!("draft-capture.recovered-{stamp}.json")]);
            assert_eq!(fs::read_to_string(root.join(&moved[0])).unwrap(), broken);
            assert!(!any_draft_in(&root), "a damaged draft still counts as active: {broken}");
        }
        let _ = fs::remove_dir_all(&root);
    }

    /// An editor draft is kept only while its dream exists. When the dream is gone, the
    /// words are moved aside, not dropped.
    #[test]
    fn an_editor_draft_is_kept_only_while_its_dream_exists() {
        let root = scratch("orphan");
        let dreams = root.join("Dreams");
        fs::create_dir_all(&dreams).unwrap();
        let draft = r#"{"name":"2026-10-13_07-12.md","text":"edited words"}"#;

        // The dream is gone: the words are kept under a recovery name
        save_draft_in(&root, "editor", draft).unwrap();
        let mut moved = Vec::new();
        set_aside_damaged_drafts_in(&root, &dreams, 2000, &mut moved).unwrap();
        assert_eq!(moved, vec!["draft-editor.recovered-2000.json".to_string()]);
        assert_eq!(fs::read_to_string(root.join(&moved[0])).unwrap(), draft);
        assert!(!any_draft_in(&root));

        // The dream exists: the draft stays active and nothing is moved
        fs::write(dreams.join("2026-10-13_07-12.md"), "the dream").unwrap();
        save_draft_in(&root, "editor", draft).unwrap();
        let mut moved = Vec::new();
        set_aside_damaged_drafts_in(&root, &dreams, 2001, &mut moved).unwrap();
        assert!(moved.is_empty());
        assert_eq!(load_draft_in(&root, "editor").unwrap().as_deref(), Some(draft));
        let _ = fs::remove_dir_all(&root);
    }

    /// Two copies moved aside in the same second never replace each other.
    #[test]
    fn recovery_names_never_replace_an_earlier_copy() {
        let root = scratch("names");
        save_draft_in(&root, "capture", "first broken").unwrap();
        let one = preserve_draft_in(&root, "capture", 5).unwrap();
        save_draft_in(&root, "capture", "second broken").unwrap();
        let two = preserve_draft_in(&root, "capture", 5).unwrap();

        assert_eq!(one.as_deref(), Some("draft-capture.recovered-5.json"));
        assert_eq!(two.as_deref(), Some("draft-capture.recovered-5_2.json"));
        assert_eq!(fs::read_to_string(root.join("draft-capture.recovered-5.json")).unwrap(), "first broken");
        assert_eq!(fs::read_to_string(root.join("draft-capture.recovered-5_2.json")).unwrap(), "second broken");
        assert_eq!(preserve_draft_in(&root, "capture", 5).unwrap(), None, "nothing left to move");
        let _ = fs::remove_dir_all(&root);
    }
}
