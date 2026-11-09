# Making a release

A release is a signed installer on the GitHub releases page. The app's updater
reads `latest.json` from the newest published release, so users get an update
only after you publish the draft.

## Before the tag

1. Make sure that CI passes on main.
2. In `CHANGELOG.md`, rename "Unreleased" to the new version, for example `## 0.2.1`. Add a new empty "Unreleased" section above it.
3. Set the new version in `Cargo.toml`, `crates/unstrung-app/tauri.conf.json` and `ui/package.json`. Run `cargo update --workspace` in the repo root and in `fuzz`, and `npm install --ignore-scripts` in `ui`, so that the lock files change too. These commands change only the version lines, not other dependencies.
4. Change the installer file name in `README.md` and `SECURITY.md` to the new version.
5. Commit, then push a tag: `git tag v0.2.1` and `git push origin v0.2.1`.

The release workflow stops if the tag does not match `tauri.conf.json`, or if `CHANGELOG.md` has no section for the version. When it passes, it makes a draft release with the installer, its `.sig` file, `latest.json` and `SHA256SUMS.txt`.

## Test the update before you publish

Users install the new version over the old one, with their data in place. Test that path on a Windows user account that is not your own, or in a Windows virtual machine. Your own account has your real chats, and an update test can change them.

1. Download the installer of the previous release from the releases page, and install it.
2. Start Unstrung. Create a character, write a few messages, and make one picture if a picture model is installed.
3. Close Unstrung. Download the new installer from the draft release, and run it over the old version.
4. Start Unstrung. Make sure that the character, the chat and the picture are there, and that a new reply works.
5. In Settings, click Copy diagnostic report. Make sure that the version is the new one and that the log has no errors from the start.
6. Close Unstrung. In Settings > Apps, uninstall it from the test account.

If a step fails, delete the draft and the tag, fix the problem, and tag again.

## Publish

1. On the releases page, open the draft and read the notes. They come from `CHANGELOG.md`.
2. Click Publish release, and make sure that "Set as the latest release" is on.
3. Open `https://github.com/Codex-Crusader/unstrung/releases/latest/download/latest.json`. Make sure that it shows the new version.
