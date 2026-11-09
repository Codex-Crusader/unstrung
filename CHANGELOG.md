# Changes

Each release lists what changed for the people who use Unstrung. When you
make a change that users notice, add a line under "Unreleased". Before a
release, rename "Unreleased" to the version number, for example "## 0.2.1".
The release workflow copies that section into the release notes, and it
stops when the section is missing.

## Unreleased

- Screens, setup steps, menus and the picture viewer now ease in instead of appearing at once. The Windows setting and Settings > Reduce motion turn this off.
- Home reminds you to make a backup when you have chats and made no backup in 30 days. Its button makes the backup right there. Settings shows the date of the last backup.
- The Pictures grid loads small copies of the pictures, so a large gallery scrolls faster and uses less memory.
- For an app that stays open for days: the daily copy of the chats, the end of "Recently deleted" after 7 days, the update check and the log size limit now work without a restart. A bug report made right after the log rotates still has the last lines. After three engine crashes in a row Unstrung still stops restarting the engine, but a crash after ten good minutes no longer counts toward that limit.
- Long chats stay smooth while a reply streams in.
- The first reply after a long pause no longer fails with a timeout on a slow PC. When the model was unloaded after 30 idle minutes, Unstrung now waits up to 5 minutes for the model to load and read the chat again. Stop still ends the wait at once.
- The chat engine's own log stays small while the app runs for weeks.
- A backup can no longer miss pictures when a library move or Free up space starts while it runs. These now ask you to wait until the backup ends. You can go on chatting while a backup runs.
- Each backup is read back and checked after it is written, so a bad write shows as an error at once.
- Automatic backup: in Settings > Storage, pick a folder, and Unstrung makes a backup there once a week and keeps the 3 newest. If the folder is missing, for example on an unplugged drive, Settings shows why and Unstrung tries again 6 hours later.
- Chats that stay in "Recently deleted" past 7 days no longer leave empty folders behind.
- Save a chat as text: on the Chats page, the download button next to a chat saves the messages that you see as a Markdown file, with the name of who wrote each one.
- Settings > Storage shows your chats in numbers: chats, messages and pictures for each character.
- Backup password: in Settings > Storage, set a password, and every backup is encrypted with it, also the automatic ones. To import such a backup, Unstrung asks for its password. If you forget the password, nobody can open the backup. Backups without a password open as before. Version 0.2.0 cannot open an encrypted backup and says that the file is not a backup that it can read.
- Dependency updates, including new versions of the encryption libraries. Files that 0.2.0 encrypted open as before.

## 0.2.0

### Before you update

- Your chats and pictures are now encrypted on disk. The first start of 0.2.0 encrypts the data that 0.1.0 saved. After that, version 0.1.0 cannot open your data, so you cannot go back to it.
- Windows keeps the key for your Windows user account on this PC. To move to a new PC, or before you reinstall Windows, go to Settings and click Export everything. On the new system, click Import backup. The backup zip is not encrypted, so keep it in a safe place.

### New

- Encryption: the chats file uses SQLCipher (AES-256), and the pictures use AES-256-GCM. The daily copies of the chats file and an opened backup are encrypted too. The window no longer keeps pictures in its disk cache.
- A stricter limit on sexual content that involves anyone under 18. Card text, persona text and your own messages now count, not only the replies.
- The window can open only the app's own pages, and folder commands accept only folders that you picked.
- Loading screens while data opens, and a crash screen with Reload and Copy details instead of a blank window.
- Snip the strings: a small game to play while a model downloads.
- Bug reports: Settings has Copy diagnostic report, and the issue form at github.com/Codex-Crusader/unstrung/issues asks for it. The report holds no chat text and no user name.

## 0.1.0

- The first public release.
