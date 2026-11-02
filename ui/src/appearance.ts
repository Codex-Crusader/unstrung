// Theme, text size and motion from Settings, applied to the whole window.

import { getCurrentWebview } from "@tauri-apps/api/webview";

export type UiSettings = Record<string, string>;

/** Page zoom per text size: it scales every size, like Ctrl and plus in a browser. */
const ZOOM = new Map([
  ["normal", 1],
  ["large", 1.12],
  ["larger", 1.25],
]);

export function applyAppearance(s: UiSettings) {
  const root = document.documentElement;
  root.dataset.theme = s.ui_theme ?? "system";
  root.dataset.motion = s.ui_motion ?? "system";
  // A failed zoom keeps the normal size; the app still works.
  void getCurrentWebview()
    .setZoom(ZOOM.get(s.ui_text_size ?? "normal") ?? 1)
    .catch(() => undefined);
}
