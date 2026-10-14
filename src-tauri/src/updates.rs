//! Automatic updates. Every time Dreamy opens, it checks GitHub for a newer stable
//! release. If there is one, it downloads and installs it, then the installer
//! relaunches Dreamy. Nothing here touches the journal: the installer only replaces
//! the program files, and dreams and tags.json live in Documents\Dreamy.
//!
//! The check runs in the background, so startup never waits for it. Every failure
//! is reported as a short status message and never blocks the window from opening.

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::UpdaterExt;

/// The tray icon's id, so its tooltip can show update progress.
pub const TRAY_ID: &str = "dreamy-tray";

/// Event that carries a new `UpdateStatus` to the window.
pub const STATUS_EVENT: &str = "update-status";

/// Longest time a check or a download may take. Offline or a stalled GitHub
/// gives up after this, and Dreamy carries on.
const TIMEOUT: Duration = Duration::from_secs(60);

/// The message shown when an update cannot be completed. The technical detail goes
/// to the console instead, so the owner sees a calm sentence.
const FAILED_MESSAGE: &str =
    "Dreamy couldn't update this time. It will try again the next time you open it.";

/// What the window's status line shows. Sent as the `update-status` event.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum UpdateStatus {
    /// Nothing to show: no update, or no check has finished yet.
    #[default]
    Idle,
    /// Downloading the new version. `percent` is `None` until the size is known.
    Downloading { percent: Option<u8> },
    /// The installer is starting. Dreamy closes and reopens on the new version.
    Installing,
    /// The update could not be completed. Dreamy keeps running.
    Failed { message: String },
}

/// The last status, shared with the window.
#[derive(Default)]
pub struct UpdateState {
    status: Mutex<UpdateStatus>,
}

impl UpdateState {
    /// The status the window should show right now.
    pub fn current(&self) -> UpdateStatus {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Remember a new status, tell the window, and update the tray tooltip.
    fn set(&self, app: &AppHandle, status: UpdateStatus) {
        if let Ok(mut current) = self.status.lock() {
            *current = status.clone();
        }
        let _ = app.emit(STATUS_EVENT, &status);
        if let Some(tray) = app.tray_by_id(TRAY_ID) {
            let _ = tray.set_tooltip(Some(tooltip_for(&status)));
        }
    }
}

/// The name of the Windows mutex that marks an update check or install in progress.
#[cfg(windows)]
const LOCK_NAME: &str = "Local\\DreamyUpdateCheck";

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn CreateMutexW(attributes: *const u8, initial_owner: i32, name: *const u16) -> isize;
    fn GetLastError() -> u32;
    fn CloseHandle(handle: isize) -> i32;
}

/// Makes sure only one update check or install runs at a time, across every running
/// copy of Dreamy. It is a named Windows mutex: a second copy started during an
/// update finds it taken and stays out. The OS frees it when the process exits, so
/// a crash can never leave updates locked.
#[cfg(windows)]
struct UpdateLock(isize);

#[cfg(windows)]
impl UpdateLock {
    /// Takes the lock, or returns `None` when another check or install holds it.
    fn take() -> Option<Self> {
        use std::os::windows::ffi::OsStrExt;

        const ERROR_ALREADY_EXISTS: u32 = 183;
        let name: Vec<u16> = std::ffi::OsStr::new(LOCK_NAME)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle == 0 {
            return None;
        }
        // The mutex already existed, so someone else holds it. Give back our handle.
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe { CloseHandle(handle) };
            return None;
        }
        Some(UpdateLock(handle))
    }
}

#[cfg(windows)]
impl Drop for UpdateLock {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

/// Dreamy is Windows-only, and so is its update flow. Other platforms have no lock.
#[cfg(not(windows))]
struct UpdateLock;

#[cfg(not(windows))]
impl UpdateLock {
    fn take() -> Option<Self> {
        Some(UpdateLock)
    }
}

/// The tray tooltip. It says "updating" while the window may be hidden.
fn tooltip_for(status: &UpdateStatus) -> &'static str {
    match status {
        UpdateStatus::Downloading { .. } | UpdateStatus::Installing => "Dreamy (updating…)",
        UpdateStatus::Idle | UpdateStatus::Failed { .. } => "Dreamy",
    }
}

/// Download progress as a whole percentage. `None` when the total size is unknown
/// or zero. Capped at 100, so a server that sends too much cannot show more.
fn percent_of(received: u64, total: Option<u64>) -> Option<u8> {
    let total = total.filter(|t| *t > 0)?;
    let percent = received.saturating_mul(100) / total;
    Some(percent.min(100) as u8)
}

/// Start the update check in the background and return at once.
/// If another check or install is already running, this does nothing.
pub fn check_in_background(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let Some(_lock) = UpdateLock::take() else {
            return;
        };
        let state = app.state::<UpdateState>();
        match check_and_install(&app).await {
            // No newer stable release: nothing to say
            Ok(()) => state.set(&app, UpdateStatus::Idle),
            Err(detail) => {
                eprintln!("Dreamy update did not complete: {detail}");
                state.set(&app, UpdateStatus::Failed { message: FAILED_MESSAGE.to_string() });
            }
        }
    });
}

/// Look for a newer release and install it if there is one. `Ok` means there was
/// nothing to install. On Windows the installer takes over and closes Dreamy, so
/// a successful install never returns here.
async fn check_and_install(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<UpdateState>();
    let updater = app
        .updater_builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;

    let Some(update) = updater.check().await.map_err(|e| e.to_string())? else {
        return Ok(());
    };

    state.set(app, UpdateStatus::Downloading { percent: None });
    let mut received: u64 = 0;
    let mut shown: Option<u8> = None;
    update
        .download_and_install(
            |chunk, total| {
                received = received.saturating_add(chunk as u64);
                // Only tell the window when the whole-number percentage changes
                let percent = percent_of(received, total);
                if percent != shown {
                    shown = percent;
                    state.set(app, UpdateStatus::Downloading { percent });
                }
            },
            || state.set(app, UpdateStatus::Installing),
        )
        .await
        .map_err(|e| e.to_string())
}

/// The window asks for the status when it opens, in case the event came first.
#[tauri::command]
pub fn get_update_status(state: State<UpdateState>) -> UpdateStatus {
    state.current()
}

/// The owner closed the failure message. Only a failure is cleared; progress is never hidden.
#[tauri::command]
pub fn dismiss_update_status(app: AppHandle, state: State<UpdateState>) {
    if matches!(state.current(), UpdateStatus::Failed { .. }) {
        state.set(&app, UpdateStatus::Idle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lock is one named mutex, so these checks run in one test: parallel tests
    /// would all see each other's lock. Only one run may hold it at a time, it is
    /// freed on drop, and a run that panics still frees it.
    #[cfg(windows)]
    #[test]
    fn only_one_update_run_at_a_time() {
        let first = UpdateLock::take().expect("first run should start");
        assert!(UpdateLock::take().is_none(), "a second run started while the first was active");
        drop(first);

        let again = UpdateLock::take().expect("the lock was not freed after the run ended");
        drop(again);

        let result = std::panic::catch_unwind(|| {
            let _run = UpdateLock::take().unwrap();
            panic!("update went wrong");
        });
        assert!(result.is_err());
        assert!(UpdateLock::take().is_some(), "a panicking run left the lock held");
    }

    /// The window reads these exact shapes, so their names must not drift.
    #[test]
    fn statuses_serialise_to_the_shape_the_window_expects() {
        let json = |s: UpdateStatus| serde_json::to_value(s).unwrap();
        assert_eq!(json(UpdateStatus::Idle), serde_json::json!({ "state": "idle" }));
        assert_eq!(json(UpdateStatus::Installing), serde_json::json!({ "state": "installing" }));
        assert_eq!(
            json(UpdateStatus::Downloading { percent: Some(40) }),
            serde_json::json!({ "state": "downloading", "percent": 40 })
        );
        assert_eq!(
            json(UpdateStatus::Downloading { percent: None }),
            serde_json::json!({ "state": "downloading", "percent": null })
        );
        assert_eq!(
            json(UpdateStatus::Failed { message: "x".into() }),
            serde_json::json!({ "state": "failed", "message": "x" })
        );
    }

    /// Progress is a whole percentage, never negative, never above 100, and absent
    /// when the size is unknown or zero.
    #[test]
    fn download_progress_is_a_clamped_whole_percentage() {
        assert_eq!(percent_of(0, Some(200)), Some(0));
        assert_eq!(percent_of(50, Some(200)), Some(25));
        assert_eq!(percent_of(199, Some(200)), Some(99));
        assert_eq!(percent_of(200, Some(200)), Some(100));
        assert_eq!(percent_of(900, Some(200)), Some(100), "a server sending too much must not overflow the bar");
        assert_eq!(percent_of(10, None), None);
        assert_eq!(percent_of(10, Some(0)), None);
        assert_eq!(percent_of(u64::MAX, Some(1)), Some(100));
    }

    /// The tray tooltip says "updating" only while an install is actually in progress.
    #[test]
    fn tooltip_reflects_update_progress() {
        assert_eq!(tooltip_for(&UpdateStatus::Idle), "Dreamy");
        assert_eq!(tooltip_for(&UpdateStatus::Failed { message: String::new() }), "Dreamy");
        assert_eq!(tooltip_for(&UpdateStatus::Downloading { percent: None }), "Dreamy (updating…)");
        assert_eq!(tooltip_for(&UpdateStatus::Installing), "Dreamy (updating…)");
    }
}
