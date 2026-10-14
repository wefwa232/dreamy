# Dreamy

A personal Windows desktop app for recording dreams and lucid dreams, the moment you wake up, before they fade.

Dreamy is built for one person's private use. It lives in the system tray, starts with Windows, and stores everything as plain Markdown files on your own PC. Nothing is hosted or shared online.

## What it does

- **Capture a dream fast.** Set the time you woke up, rate lucidity from 0 to 4, add tags, and write the dream. `Ctrl + Enter` saves it.
- **Tags with style.** Tags have a name, a colour, and a symbol. Your tag library is saved separately, so tags can be reused.
- **Three visual worlds.** Deep night sky, dawn fading (follows the time of day), and a lucid instrument look. Ambient motion can be switched off in settings.
- **Read past dreams.** Click a recent dream to read it, or open the **Dreams** sidebar.
- **Edit and delete past dreams.** In the Dreams sidebar, right-click a dream to edit it, rename it by date and time, or delete it. Hold a dream to select several and delete them together.
- **The Dreams sidebar.** Move the mouse to the left edge of the window and it slides out. Search by words, tags, or dates, sort by date or lucidity, and filter by lucidity level or tags.
- **Voice input** comes from your own dictation tool, Handy (`Ctrl + Space`). Dreamy has no speech system of its own.
- **Closing the window** hides Dreamy to the system tray. Quit it from the tray menu.

## Where your dreams are kept

- Dreams: `Documents\Dreamy\Dreams`, one Markdown file per dream
- Tag library: `Documents\Dreamy\tags.json`

Each dream file starts with a short header (date, lucidity, tags), followed by the dream text. You can open these files in any text editor.

## Running it

You need Windows, [Node.js](https://nodejs.org), and [Rust](https://rustup.rs) (the Tauri build tools).

From the project folder:

```bash
npm install
npm run dev
```

This opens a development window. To build an installable app instead, run `npm run build`.

Close any running Dreamy window before rebuilding, or the build cannot replace the running program.

## Project documents

- [PLAN.md](PLAN.md): what is built, what is next, and open questions
- [PRODUCT.md](PRODUCT.md): who Dreamy is for and the principles behind it
- [CLAUDE.md](CLAUDE.md): notes for working on this project with Claude

## Project layout

```
src/            the screens: index.html, styles.css, app.js
src-tauri/      the Windows side in Rust: files, tray icon, startup
PLAN.md         status and roadmap
PRODUCT.md      product notes
```

## Status

The foundation, visual worlds, tag system, reading past dreams, the Dreams sidebar, and editing and deleting past dreams are built. Editing covers the words, lucidity, tags, and date. Deleting moves dreams to `Documents\Dreamy\Deleted`, so nothing is destroyed. Still to come: reality-check reminders, a wake-back-to-bed timer, and a patterns view. See [PLAN.md](PLAN.md).

---

Private project. Not for distribution.
