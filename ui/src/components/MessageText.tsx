// Shows message text as text. Actions in *asterisks* show in italic.
// When a message puts speech in quotes, the text outside the quotes is
// description, even when the model left out the asterisks.
// No HTML, no links, no images: model output and imported cards are untrusted.

import type { ReactNode } from "react";

const ACTION = /(\*[^*\n]+\*)/g;
const SPEECH = /("[^"\n]*"|“[^”\n]*”)/g;

/** Splits text into *actions* and the rest. `rest` is the class for the rest. */
function actions(text: string, rest: string, key: string): ReactNode[] {
  return text.split(ACTION).map((part, i) =>
    part.length > 2 && part.startsWith("*") && part.endsWith("*") ? (
      <em key={`${key}-${String(i)}`} className="action">
        {part.slice(1, -1)}
      </em>
    ) : (
      // A lone asterisk is left from an action that a quote split.
      <span key={`${key}-${String(i)}`} className={rest || undefined}>
        {rest === "narration" ? part.replace(/\*/g, "") : part}
      </span>
    ),
  );
}

export function MessageText({ text }: { text: string }) {
  const parts = text.split(SPEECH);
  const quoted = parts.length > 1;
  return (
    <p className="message-text">
      {quoted
        ? parts.map((part, i) =>
            i % 2 === 1 ? (
              // Asterisks inside speech are a model mistake. They still show as an action.
              <span key={i} className="speech">
                {actions(part, "", String(i))}
              </span>
            ) : (
              actions(part, "narration", String(i))
            ),
          )
        : actions(text, "", "t")}
    </p>
  );
}
