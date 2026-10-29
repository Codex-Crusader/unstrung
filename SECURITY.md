# Security policy

## Report a security bug

Do not open a public issue for a security bug. Use GitHub private vulnerability reporting: open the **Security** tab of this repository and click **Report a vulnerability**.

Include the version, the steps to reproduce the bug, and what an attacker can do with it. You will get an answer within 7 days.

## What Unstrung protects

Unstrung runs all chat on your computer. The design follows these rules:

- The AI has no power. The chat model has no tools. It cannot run commands, read or write files, or open the network. Its output is untrusted text, and the app shows it only as text.
- The window can call only a short list of typed commands. It has no shell, no general file access, and no network access. A strict Content Security Policy blocks remote scripts, styles and images.
- The chat engine (Ollama) listens only on `127.0.0.1`, on a random free port, with its own models folder. Other devices on the network cannot reach it.
- The core builds every file path from a fixed root folder. Paths never come from the window.
- The database uses parameterized queries only.
- The project crates forbid `unsafe` code.

## What Unstrung does not protect

- A person or program with administrator access to your PC, or with your Windows password, can read your data. No app can stop that.
- Data is not encrypted on disk in version 1. Your Windows account protects it.
- Other programs that run as your Windows user can find the local engine port and send requests to it. The random port makes this harder. It does not make it impossible.

## Supported versions

Only the newest release gets security fixes.
