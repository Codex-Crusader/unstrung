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
- Data is not encrypted on disk in version 1. Your Windows account protects it. To protect it if someone takes your PC or its disk, turn on BitLocker or Device encryption in Windows.
- Other programs that run as your Windows user can find the local engine port and send requests to it. Such programs can also read your data files, so the port does not give them more access.

## How the project checks its dependencies

- Every push and a weekly run check the Rust crates with `cargo audit` and `cargo deny`, and the UI packages with `npm audit`.
- Dependabot opens pull requests for new versions and for security fixes.
- A fuzz test runs the character card parser with random input on every push.

## Supported versions

Only the newest release gets security fixes.
