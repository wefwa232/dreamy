# Releasing Dreamy and automatic updates

This explains how Dreamy updates itself, how to publish a new version, and which
parts must be kept safe for good.

## How updates work

- Every time Dreamy opens, it asks GitHub whether a newer **stable** release exists.
  Drafts and pre-releases are ignored, so only finished releases reach your PC.
- If there is nothing newer, Dreamy opens as usual and says nothing.
- If there is a newer version, Dreamy downloads it, shows "Updating Dreamy…" in a
  small line, and installs it. Then it reopens on the new version.
- **Unsaved words are never lost to an update.** If a dream is half written on the
  capture card, or an edit is open and unsaved, the update waits. The line then says
  "Update ready. It will install once your unsaved words are saved." Save the dream
  or clear the card, and the update installs. Words on the card or in an edit are
  also kept on disk, so a restart brings them back.
- The install is approved only after the words are settled. Dreamy checks the card and the
  editor once every pending save has been written, then holds them shut for the moment the
  installer starts. Typing that begins after the download finishes is saved, and the update
  waits for it.
- If a kept copy of unsaved words is damaged, or belongs to a dream that no longer exists,
  Dreamy does not drop it. It moves the copy to `Documents\Dreamy` under a name such as
  `draft-capture.recovered-….json` and says so on the card. Nothing is deleted.
- If the PC is offline, GitHub is unavailable, or the download fails, Dreamy still
  opens normally. A small note says the update could not be completed, and it tries
  again the next time you open Dreamy.
- **Your dreams and tags are never touched.** They live in `Documents\Dreamy`.
  An update only replaces the program files in `%LOCALAPPDATA%\Dreamy`.
- Every update must be signed with the private key described below. Dreamy checks
  that signature and refuses anything it cannot verify.

## One-time step before the first updater release

Copies of Dreamy 0.1.0 have no updater inside them, so they cannot update
themselves. Install **0.1.1** once, by hand, from its GitHub Release (the
`Dreamy_0.1.1_x64-setup.exe` file). From then on, every release updates itself.

## Publishing a new version

1. Choose a number higher than the last release (for example 0.1.1 → 0.1.2).
2. On a branch, set it everywhere in one step:

   ```bash
   npm run bump -- 0.1.2
   ```

3. Commit the change, open a pull request, and merge it into `main`.
4. On `main`, tag the release and push the tag:

   ```bash
   git tag v0.1.2
   git push origin v0.1.2
   ```

5. GitHub Actions does the rest. It checks the version numbers, runs the checks
   (the JavaScript syntax check, the Rust tests and the Rust build check), builds the
   Windows installer, signs it, and publishes the release with its `latest.json` file.
   Follow it on the repository's **Actions** tab. It usually takes 10 to 20 minutes.
6. Check the **Releases** page: the new version should be marked **Latest**.

Only plain version tags such as `v0.1.2` are accepted. A tag with a suffix, such as
`v0.2.0-beta.1`, is refused, so no unfinished build can reach installed copies.

Nothing is published if any of these stop the run:

- the tag is not a plain version, or
- any of the five version records does not match the tag. They are `package.json`,
  both version entries in `package-lock.json`, `src-tauri/tauri.conf.json`,
  `src-tauri/Cargo.toml` and Dreamy's entry in `src-tauri/Cargo.lock`. `npm run bump`
  sets all five, or
- the tag is not higher than the latest stable release, or
- a check fails. The checks run before the key is used, so a failing build is never signed.

## Signing keys: what to keep safe

There is one key pair. The **private key** signs each update. The **public key**
lets Dreamy check the signature.

| Item | Where it lives | Rule |
| --- | --- | --- |
| Private key file | `C:\Users\User\.tauri\dreamy-updater.key` (outside the project folder) | Keep a backup somewhere safe, such as an encrypted drive. Never commit it, paste it into chat, or put it in a file in the project. |
| Private key password | Your password manager | Keep it with the key. |
| GitHub secret `TAURI_SIGNING_PRIVATE_KEY` | Repository settings, Secrets and variables, Actions | A copy of the key file's contents, used by the build only. |
| GitHub secret `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Same place | A copy of the key's password. |
| Public key | `src-tauri/tauri.conf.json` (`plugins.updater.pubkey`) | Safe to share. Committed to the repository. |

**Permanent warning.** If the private key or its password is lost, or the key is
replaced, installed copies of Dreamy can no longer be updated automatically. Each
of them would need a manual reinstall. Keep the key and its password safe for as
long as Dreamy is in use.

Do not put any of these in the repository, an issue, a pull request, a chat, or
a log. The `.gitignore` file blocks the usual key file names as a safety net.

## If something goes wrong

- **The Actions run fails at the version check:** a version number in the project
  does not match the tag, the tag has a suffix, or the tag is not higher than the
  latest stable release. Fix the number with `npm run bump`, then tag again with a
  new number.
- **The Actions run fails at the checks:** a test or check did not pass, so nothing
  was built, signed or published. Fix the problem, then tag again with a new number.
- **The build fails at signing:** the two GitHub secrets are missing or wrong.
  Check the secret names (they are case-sensitive) and that the password matches.
- **Installed copies do not update:** check that the release is marked **Latest**
  and that its assets include `latest.json`. Dreamy's note will say when a check
  fails, and it will try again on the next launch.
