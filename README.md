# Unstrung

Your characters. Your computer. Nothing leaves it.

Unstrung is a desktop app for roleplay chat with AI characters. All processing is local. The app does not send chats, pictures or telemetry to the cloud.

Status: Phase 1 of 8 (chat core). The app opens one built-in character, Ren Kagami, and streams replies from a local model. The first-run setup, the model download and the installer come in later phases. Until then, the app needs a developer setup.

## Requirements

- Windows 10 or 11, 64-bit
- [Ollama](https://ollama.com) installed for your Windows user. Phase 4 replaces this with an engine download inside the app.
- The model `qwen2.5:7b` (about 4.7 GB)
- For a build: Rust (stable), Node.js 22 or newer, and the Visual Studio C++ build tools

## Build and run

1. Get the model with your own Ollama:

   ```
   ollama pull qwen2.5:7b
   ```

2. Build the UI. The app embeds it, so you must build it before the Rust code.

   ```
   cd ui
   npm ci
   npm run build
   cd ..
   ```

3. Build and start the app:

   ```
   cargo run -p unstrung-app
   ```

The app starts its own private Ollama process on a random port on `127.0.0.1`. It does not use or change the Ollama that you run yourself.

A debug build reads models from your own Ollama folder (`%USERPROFILE%\.ollama\models`), so you do not download them twice. A release build uses its own models folder inside the app folder. To point a build at another models folder, set `UNSTRUNG_DEV_OLLAMA_MODELS`.

## Where your data is

| Folder | Holds |
| --- | --- |
| `%LOCALAPPDATA%\Unstrung` | The database (`app.db`), profile pictures, logs |

To use a different folder, for example for a test, set `UNSTRUNG_HOME` to that folder.

## Tests

```
cargo test --workspace
```

These tests need the chat engine and the model, so they do not run by default:

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
