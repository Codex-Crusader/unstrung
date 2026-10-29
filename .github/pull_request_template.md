## What this changes

## Security checklist

- [ ] New input from outside the core (UI command, file, model output, engine reply)? It has a type, a size limit and a check.
- [ ] New file access? The core builds the path from a root folder and an ID.
- [ ] New dependency? Give the reason, the license and the maintainers.
- [ ] New IPC command? It is in `build.rs` and in `capabilities/default.json`.
