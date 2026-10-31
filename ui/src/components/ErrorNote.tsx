import { useEffect, useState, type ReactNode } from "react";
import { openWindowsSecurity, setupState, type UiError } from "../api";
import { strings } from "../strings";
import { Icon } from "./Icon";

/**
 * An error in plain words: what happened, one action, and the technical
 * detail behind "Details". `lead` replaces the message when the screen knows
 * better what failed, for example "The chat could not open."
 */
export function ErrorNote({ error, lead, children }: { error: UiError; lead?: string; children?: ReactNode }) {
  const [open, setOpen] = useState(false);
  const message = lead ?? error.message;
  const detail = error.detail && error.detail !== message ? error.detail : "";
  return (
    <div className="chat-error" role="alert">
      <Icon name="warning" />
      <span>{message}</span>
      {children}
      {detail && (
        <button
          type="button"
          className="link-button"
          aria-expanded={open}
          onClick={() => {
            setOpen((v) => !v);
          }}
        >
          {open ? strings.status.hideDetails : strings.status.details}
        </button>
      )}
      {open && <code className="error-detail">{detail}</code>}
      {error.tip === "antivirus" && <AntivirusTip />}
    </div>
  );
}

/** The models folder to exclude, with a copy button and a way to Windows Security. */
function AntivirusTip() {
  const s = strings.antivirus;
  const [folder, setFolder] = useState("");
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    void setupState().then((state) => {
      setFolder(state.models);
    });
  }, []);
  return (
    <div className="antivirus-tip">
      <p>{s.tip}</p>
      <code className="folder-path">{folder}</code>
      <div className="safe-actions">
        <button
          type="button"
          className="button button-quiet"
          disabled={!folder}
          onClick={() => {
            void navigator.clipboard.writeText(folder).then(() => {
              setCopied(true);
            });
          }}
        >
          {copied ? s.copied : s.copy}
        </button>
        <button
          type="button"
          className="button button-quiet"
          onClick={() => {
            void openWindowsSecurity();
          }}
        >
          {s.open}
        </button>
      </div>
    </div>
  );
}
