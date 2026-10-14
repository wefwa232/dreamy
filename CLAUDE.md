# Working on Dreamy

Read this first, then [PLAN.md](PLAN.md) for the current status.

## What this is

Dreamy is a personal Windows desktop dream journal built with Tauri 2. The interface is plain HTML, CSS, and JavaScript in `src/`. The Rust side in `src-tauri/` handles files, the tray icon, and start-with-Windows.

## Who it's for

The owner is not a developer and does not read code. They want Claude to build and maintain the app. In replies:

- Explain choices in plain language. Keep code details out unless asked.
- Address the owner as "sir" (set by the global Jarvis persona).
- Put UI and feel first. The owner cares most about how it looks and behaves.

## Working rules

- Keep it desktop-only and lightweight. No mobile, no web hosting, no separate voice system.
- Dreams are plain Markdown files in `Documents\Dreamy\Dreams`. Never delete or rewrite the owner's real dream files. Treat that folder as the owner's data.
- Every colour must come from a theme token in `styles.css`, so all three visual worlds (Night, Dawn, Lucid) keep working.
- Respect the existing motion setting: animations are switched off when ambient motion is off.
- Update `PLAN.md` status as features land.
- Before a rebuild, the running Dreamy window must be closed, or `cargo build` cannot replace the `.exe`.
- Commit and push only when the owner asks.

## Where things live

- `src/index.html`: the screen layout (capture card, recent list, reader sheet, Dreams sidebar, settings drawer)
- `src/app.js`: all behaviour. Sections are marked with headings: backend bridge, tags, saving, recent list, reader, sidebar, sky canvas, settings, start.
- `src/styles.css`: theme tokens at the top, then components.
- `src-tauri/src/main.rs`: file commands (`save_dream`, `list_dreams`, `read_dreams`, `read_dream`, `load_tags`, `save_tags`), tray, and startup.
- `src-tauri/src/updates.rs`: automatic updates (check at startup, download, install, status messages). Release steps and key handling are in [docs/releasing.md](docs/releasing.md).
- `.github/workflows/release.yml`: builds, signs and publishes a release when a `v*` tag is pushed.
- `scripts/bump-version.js`: `npm run bump -- 0.1.2` sets the version in every file that records it.

## Checking changes

- Syntax check the JavaScript: `node --check src/app.js`
- Compile check the Rust: `cargo check` inside `src-tauri/`
- The in-app browser cannot load the styles from the project folder, so visual checks need the real window (`npm run dev`).
