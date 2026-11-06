# Security policy

## Report a security bug

Do not open a public issue for a security bug. Use GitHub private vulnerability reporting: open the **Security** tab of this repository and click **Report a vulnerability**.

Include the version, the steps to reproduce the bug, and what an attacker can do with it. You will get an answer within 7 days.

## Check your download

The installer does not have a code signing certificate yet. Windows SmartScreen shows "Windows protected your PC" for it. Click More info, then Run anyway.

Because Windows cannot check the publisher, check the file yourself. Each release has a `SHA256SUMS.txt` file with the SHA-256 hash of the installer. To get the hash of your download, run this command in PowerShell:

```
Get-FileHash .\Unstrung_0.1.0_x64-setup.exe -Algorithm SHA256
```

In Command Prompt, use `certutil -hashfile Unstrung_0.1.0_x64-setup.exe SHA256`. If the hash is not the same as the one in `SHA256SUMS.txt`, do not run the file. Report it as a security bug.

Updates inside the app do not depend on this check. The app installs an update only when it carries a valid signature from the project's update key, for the version that it announces.

## What Unstrung protects

Unstrung runs all chat on your computer. The design follows these rules:

- The AI has no power. The chat model has no tools. It cannot run commands, read or write files, or open the network. Its output is untrusted text, and the app shows it only as text.
- The window can call only a short list of typed commands. It has no shell, no general file access, and no network access. A strict Content Security Policy blocks remote scripts, styles and images. The window cannot go to another page or open a second window.
- The window can name a folder only if you picked that folder in the Windows folder picker, or if it is one of the app's own folders. Other file paths never come from the window. The core builds them from a fixed root folder.
- The chat engine (Ollama) listens only on `127.0.0.1`, on a random free port, with its own models folder. Other devices on the network cannot reach it. Settings from your own Ollama do not reach it. The engine stops when the app stops, also after a crash.
- Downloads go only to the hosts in the app's catalog, over HTTPS. Each downloaded file must match a SHA-256 hash from the catalog, or the app deletes it.
- Sexual content that involves anyone under 18 is never allowed, whatever the settings. Adult content needs an 18+ character and a persona with an adult age, from the database, not from the model. A chat stays general when the card, the persona, the messages or the memory notes mention someone under 18, also when the age is written as a word. The app refuses such text in your own messages, stops a reply that crosses the line, and checks every picture prompt.
- Your chats and pictures are encrypted on disk. The chats file uses SQLCipher (AES-256). The pictures use AES-256-GCM. One random key encrypts both, and it is in `data.key` in the app folder. Windows (DPAPI, the Data Protection API) protects that file for your Windows user only, so another Windows user, another PC, or a copy of the disk cannot open your data. The daily copies of the chats file are encrypted too. The window never keeps a picture in its disk cache. The first start after the update removes the cache that version 0.1.0 made.
- The database uses parameterized queries only.
- The project crates forbid `unsafe` code.

## What goes over the network

| When | What | From |
| --- | --- | --- |
| First start | The chat engine | github.com |
| First start | One chat model | the Ollama library |
| If you turn on pictures | The picture engine and a picture model | github.com, huggingface.co |
| A minute after start | A check for a newer version. You can turn it off in Settings. | github.com |
| When you click Update | The new installer | github.com |

Your chats, characters, personas, pictures and usage data never go over the network. The app has no telemetry.

## What Unstrung does not protect

- A person or program with administrator access to your PC, or with your Windows password, can read your data. No app can stop that.
- Any program that runs as your Windows user can ask Windows to open the data key. Encryption protects your data from other users and from a stolen disk, not from malware on your own account.
- A backup zip that you export is not encrypted, so that it opens on a new PC. Keep it in a safe place. A picture that you save outside the app is a plain PNG file.
- The picture engine works with plain files. It gets a plain copy of the pictures it needs in the `work` folder, and the app deletes the copy after each picture. After a crash, the next start deletes it.
- If an administrator resets your Windows password, Windows can lose the key that opens `data.key`, and your chats do not open. Unstrung never makes a new key over your data. To get your data back, sign in as the user who made it with the old password, or import a backup zip. Export a backup now and then.
- Deleted data can stay on the disk until other files use the space. Version 0.1.0 saved data plain, so traces of it can stay after the update. For full protection, turn on BitLocker or Device encryption in Windows.
- Other programs that run as your Windows user can find the local engine port and send requests to it. Such programs can also read your data files, so the port does not give them more access.

## How the project checks its dependencies

- Every push and a weekly run check the Rust crates with `cargo audit` and `cargo deny`, and the UI packages with `npm audit`.
- Dependabot opens pull requests for new versions and for security fixes.
- A fuzz test runs the character card parser with random input on every push.

## Supported versions

Only the newest release gets security fixes.
