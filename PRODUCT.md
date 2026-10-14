# Product

<!-- impeccable:product-schema 1 -->

## Platform

Windows desktop app (not a website).

## Stack

Tauri 2 desktop app. The interface is plain HTML, CSS, and JavaScript in `src/`. A small Rust backend in `src-tauri/` handles files, the tray icon, and startup.

## Users

One user: the owner. Used personally, first thing after waking, on a Windows PC. The user is not a developer and does not read code.

## Product Purpose

A personal journal for dreams and lucid dreams. Success means capturing a dream in under a minute before it fades, and over time seeing patterns in recurring tags and lucidity.

## Positioning

A dream journal that lives on the desktop, stores everything as plain local files, and accepts voice through the user's own local dictation tool (Handy). Nothing is hosted or shared.

## Operating Context

- Used right after waking, often half-awake, so capture must be quick and forgiving.
- Voice is dictated with Handy (Ctrl+Space). Handy pastes into whatever field has focus, so the main text field must always accept paste and typing.
- The app starts with Windows and lives in the system tray. Closing the window hides it to the tray.

## Capabilities and Constraints

- Dreams are saved as Markdown files in Documents\Dreamy\Dreams, with date, lucidity (0 to 4), and tags at the top.
- Tags live in a library file (Documents\Dreamy\tags.json). Each tag has a name, a color, and a symbol.
- Past dreams open in a reader. A Dreams sidebar (opened from the left edge) searches, sorts, and filters them by lucidity and tags.
- Past dreams can be edited, renamed by date and time, and deleted. Deleted dreams move to a Deleted folder, so nothing is lost for good. Holding a dream selects several at once.
- Planned, not yet built: reality-check reminders from the tray, a wake-back-to-bed timer, and a patterns view.
- Low resource use is a requirement: the tray process should stay small on RAM and CPU.

## Brand Commitments

None. This is a private tool. The name is Dreamy.

## Evidence on Hand

No real customers or content. Sample entries are the owner's own test dreams.

## Product Principles

- Capture beats polish. Saving must be fast and never lose words.
- Local and private. Files stay on the user's machine.
- Quiet by default. The interface should calm the mind, not compete with the dream.
- Expandable. New features (reminders, patterns) should fit without a rewrite.

## Accessibility & Inclusion

Respect the system's reduced-motion setting by default. Keep text contrast readable in all three visual worlds.
