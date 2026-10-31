import { useState, type ReactNode } from "react";
import type { UiError } from "../api";
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
    </div>
  );
}
