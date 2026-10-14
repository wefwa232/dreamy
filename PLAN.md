# Dreamy: Planning Notes

A personal Windows desktop app for journaling dreams and lucid dreams. Built for one user (the owner). The owner does not read code: Claude builds and maintains everything.

## Decisions

- **Platform:** Windows desktop app, a real installed-style application (not a website or web app).
- **Audience:** just the owner for now. Public release is a possible later step.
- **Tech stack:** Tauri 2. The screens are HTML, CSS, and JavaScript in `src/` (split into `index.html`, `styles.css`, `app.js`). A small Rust backend in `src-tauri/` handles files, the tray icon, and startup. Chosen for low RAM use and easy expansion.
- **Storage:** local files only, nothing hosted online.
  - Each dream is a Markdown file in `Documents\Dreamy\Dreams`, with date, lucidity (0 to 4), and tags at the top.
  - The tag library is `Documents\Dreamy\tags.json`: each tag has a name, colour, and symbol.
- **Voice input:** handled by Handy. Ctrl+Space, speak, and Handy pastes into the focused field. Dreamy has no speech system of its own.
- **Startup:** Dreamy starts automatically with Windows, minimized to the tray. The app re-enables this on each launch.
- **Closing the window:** hides Dreamy to the tray. Quit is in the tray menu.
- **Visual worlds:** three themes, chosen in the settings panel (gear, top left) and remembered per PC:
  - *Deep night sky* (default): starfield and moonlight.
  - *Dawn fading*: dusk-to-rose sky that follows the real time of day.
  - *Lucid instrument*: teal grid, scanning light, clinical edge.
- **Motion:** ambient motion (starfield, cursor glow, drifting light) can be switched off in settings. Reduced-motion is respected by default. Interactions (pill pop-ins, save star burst, button ripples) are part of the design.
- **Tags:** typing a tag and pressing Enter adds a pill. The ✦ button opens the designer (name, one of eight colours, one of twelve symbols or a plain dot). Existing tags appear as chips while the field is focused. Click a pill to restyle it; Backspace removes the last one.
- **Reality-check prompts:** planned as interval tray notifications. Default idea: about every 30 minutes during waking hours, with quiet hours and a randomized interval.
- **Footprint goal:** idle RAM well under 100 MB, with no background polling.

## Status

- [x] Foundation: window, capture form, tray icon, close-to-tray, start with Windows, save to Markdown, recent entries.
- [x] Visual worlds: three themes with a settings panel and an ambient motion switch.
- [x] Tag system: pills, designer (colour and symbol), library chips, saved to `tags.json`.
- [x] Open and view past entries: click a recent dream (or "Show all") to read it in a reader sheet with its date, lucidity, and tags.
- [x] Dreams sidebar: slides out when the pointer reaches the left edge. The Dreams tab opens a journal with search (words, tags, dates), sort, lucidity filter, and tag filter.
- [x] Data safety: a save never overwrites an existing dream (a "_2" or "_3" is added when two share a minute), and dreams and the tag library are written to a temporary file first so a crash can't leave a half-written entry. Covered by a Rust test that uses a scratch folder.
- [x] Second safety pass: a dream is refused unless its date is valid (an empty date no longer produces a bad filename). Dream files that can't be read are named in a status message instead of vanishing. A `tags.json` that can't be read is never overwritten: tag changes are refused until it is fixed. Saving is blocked while a save is running.
- [x] Atomic saves: dreams and the tag library are written to a temporary file, flushed to disk, and then moved into place. A new dream still never replaces an existing one. Covered by Rust tests that simulate a failed write.
- [x] Tag designer title: the CSS now targets the `h2` the markup uses, so the 14px title is applied.
- [ ] **Dreams folder move (planned, not built).** The app writes to `C:\Users\User\Documents\Dreamy`, but Windows reports Documents as `G:\User\Documents`. The safe copy-first plan is in [docs/dreams-folder-migration.md](docs/dreams-folder-migration.md). Nothing moves until the owner decides.
- [ ] **Automatic updates (built on branch `automatic-updates`, not yet live).** Each launch checks GitHub for a newer stable release, installs it quietly, and reopens. Failures never block startup. Still to do: create the signing key pair, add the two GitHub secrets, paste the public key into `tauri.conf.json`, and run the end-to-end test with scratch test releases. Owner must install 0.1.1 by hand once. Steps in [docs/releasing.md](docs/releasing.md).
- [ ] Also open: Content Security Policy, startup always re-enabled, and the Dreams sidebar only opening on hover.
- [ ] **Verify in the real desktop window.** The latest build was checked in a browser preview and `cargo check` passes, but `cargo build` was blocked because the running dev copy holds `dreamy.exe`. Close Dreamy first, then `npm run dev`.
- [ ] Reality-check reminders (interval notifications from the tray)
- [ ] Wake-back-to-bed timer
- [ ] Patterns view (frequency, lucidity over time, tag correlations)
- [x] Edit past entries: right-click a dream in the Dreams sidebar and choose Edit (words, lucidity, tags, date and time) or Rename (changes the date and time, which is also the file name). Saves replace the file in place, or move it to a new name without replacing another dream.
- [x] Select and delete dreams: hold a dream for half a second to select several, then use Select all, Delete or Done in the bar at the bottom of the sidebar. Right-click offers Open, Edit, Rename, Select and Delete. Delete always asks first, and the dreams move to `Documents\Dreamy\Deleted` rather than being destroyed. Rust tests cover this in scratch folders. Checked in a browser preview with stand-in storage, not yet in the desktop window.

## Running it

From the DREAMY folder, run `npm run dev` for a development window, or `npm run build` for an installable Windows app. Close any running Dreamy window before rebuilding, or the build can't replace the running `.exe`.

## Open questions

- Confirm Handy pastes into the Dreamy capture box (expected yes).
- Reminder interval and quiet hours defaults.
- Whether the Dawn world should keep following the real time of day, or stay fixed.
