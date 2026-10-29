// Tauri isolation hook. Every IPC message from the UI passes through here,
// in a separate sandboxed frame, before Tauri encrypts it for the core.
// Messages that are not plain objects with a string command are dropped.
window.__TAURI_ISOLATION_HOOK__ = (payload) => {
  if (payload === null || typeof payload !== "object" || typeof payload.cmd !== "string") {
    return null;
  }
  return payload;
};
