# Unstrung

Your characters. Your computer. Nothing leaves it.

Unstrung is a desktop app for roleplay chat with AI characters. All processing is local. The app does not send chats, pictures or telemetry to the cloud.

Status: Phase 4 of 8 is done. A new Windows PC goes from the installer to a first chat with no terminal. Pictures come in Phase 5.

## Install

1. Run `Unstrung_0.1.0_x64-setup.exe`. It installs for your Windows user only and needs no administrator rights. The installer is not signed yet, so Windows SmartScreen can show "Windows protected your PC". To go on, click More info, then Run anyway.
2. Start Unstrung. The welcome flow asks where to keep your pictures and models, then downloads two things:
   - The chat engine (Ollama 0.40.13), about 1.6 GB. AMD graphics cards get an extra 0.26 GB.
   - One chat model, 3.3 GB to 6.6 GB. The app suggests a model that fits your PC.
3. Pick a persona or skip that step, then start a chat.

You can close Unstrung during the downloads. The next start goes on from where it stopped.

## Requirements

- Windows 10 or 11, 64-bit
- About 10 GB of free disk space for the engine and one model
- A graphics card is optional. Without one the chats work, but replies are slow.

## Where your data is

| Folder | Holds |
| --- | --- |
| `%LOCALAPPDATA%\Unstrung` | The chats database (`app.db`), profile pictures, logs, the chat engine, and copies of the database from before each update that changes it (`backups`) |
| The library folder that you picked in the welcome flow | Pictures (Phase 5). By default it is inside the app folder. |
| The models folder that you picked in the welcome flow | The chat models. By default it is inside the app folder. |

The database always stays in the app folder, so a sync tool such as OneDrive cannot damage it.

## When something goes wrong

The Start menu has a shortcut named "Unstrung (Safe start)". It opens the app without the chat engine and offers Repair: a quick check, or a full check that also tests every model file. If the app fails to start two times in a row, the next start opens safe start by itself.

## Uninstall

Use Settings, Apps, Installed apps. The uninstaller asks if it must also delete your chats, characters, pictures and models. If you leave that box clear, your data stays, and a new install finds it again.

## Build from source

You need Rust (stable), Node.js 22 or newer, and the Visual Studio C++ build tools.

1. Build the UI. The app embeds it, so you must build it before the Rust code.

   ```
   cd ui
   npm ci
   npm run build
   cd ..
   ```

2. Build and start the app:

   ```
   cargo run -p unstrung-app
   ```

3. To make the installer, install the Tauri CLI (`cargo install tauri-cli --version 2.12.4 --locked`), then run this:

   ```
   cd crates/unstrung-app
   cargo tauri build
   ```

   The installer is in `target/release/bundle/nsis`.

The app starts its own private Ollama process on a random port on `127.0.0.1`. It does not use or change the Ollama that you run yourself.

A debug build reads models from your own Ollama folder (`%USERPROFILE%\.ollama\models`), so you do not download them twice. If the app folder has no engine, a debug build also uses your installed Ollama program. A release build uses only its own engine and its own models folder. To point a build at another models folder, set `UNSTRUNG_DEV_OLLAMA_MODELS`. To use a different app folder, for example for a test, set `UNSTRUNG_HOME`.

## Tests

```
cargo test --workspace
```

These tests need the chat engine and a model, so they do not run by default:

```
cargo test -p unstrung-core --test live_chat -- --ignored --nocapture
```

The app also has a self-test. It sends one message through the real window and command path, writes `logs/self-test.json`, and exits with code 0 (pass) or 1 (fail). Run it two times on an empty folder. The second run must open with the history of the first run.

```
set UNSTRUNG_HOME=%TEMP%\unstrung-test
target\debug\unstrung-app.exe --self-test --background
```

`--background` opens the window without focus.

## Security

See [SECURITY.md](SECURITY.md).

## License

MIT. Copyright Bhargavaram Krishnapur. See [LICENSE](LICENSE).

Tauri needs five crates under MPL-2.0 (`cssparser`, `cssparser-macros`, `dtoa-short`, `option-ext`, `selectors`). Unstrung uses them unchanged. `deny.toml` lists them by name.
