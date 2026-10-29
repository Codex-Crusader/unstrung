import { useState } from "react";
import { stopReply, testReply, toUiError, type CharacterFields, type TestLine } from "../api";
import { strings } from "../strings";
import { MessageText } from "./MessageText";

/** A chat with the character as it is in the form now. Nothing is saved. */
export function TestChat({
  fields,
  ready,
  onClose,
}: {
  fields: CharacterFields;
  ready: boolean;
  onClose: () => void;
}) {
  const [lines, setLines] = useState<TestLine[]>(() => [{ role: "character", content: fields.greeting }]);
  const [streaming, setStreaming] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const busy = streaming !== null;

  const send = (text: string) => {
    if (!text.trim() || busy || !ready) return;
    const history: TestLine[] = [...lines, { role: "user", content: text }];
    setLines(history);
    setDraft("");
    setError(null);
    setStreaming("");
    testReply(fields, null, history, (event) => {
      switch (event.kind) {
        case "text":
          setStreaming((s) => (s ?? "") + event.text);
          break;
        case "restart":
          setStreaming("");
          break;
        case "done":
          setStreaming(null);
          if (event.text) setLines((l) => [...l, { role: "character", content: event.text }]);
          break;
        case "failed":
          setStreaming(null);
          setError(event.error.message);
          break;
      }
    }).catch((e: unknown) => {
      setStreaming(null);
      setError(toUiError(e).message);
    });
  };

  return (
    <aside className="test-chat" aria-label={strings.creator.test}>
      <div className="chat-list-head">
        <h2>{strings.creator.test}</h2>
        <button type="button" className="link-button" onClick={onClose} disabled={busy}>
          {strings.creator.testClose}
        </button>
      </div>
      <p className="field-hint">{strings.creator.testHint}</p>
      <div className="test-lines" role="log">
        {lines.map((line, i) => (
          <div key={i} className={`test-line test-${line.role}`}>
            <span className="speaker">{line.role === "user" ? strings.chat.you : fields.name}</span>
            <MessageText text={line.content} />
          </div>
        ))}
        {busy && (
          <div className="test-line test-character" aria-busy="true">
            <span className="speaker">{fields.name}</span>
            <MessageText text={streaming || "..."} />
          </div>
        )}
        {error && (
          <p className="chat-error" role="alert">
            {error}
          </p>
        )}
      </div>
      <div className="test-ready">
        {strings.creator.testLines.map((line) => (
          <button
            key={line}
            type="button"
            className="chip"
            disabled={busy || !ready}
            onClick={() => {
              send(line);
            }}
          >
            {line}
          </button>
        ))}
      </div>
      <form
        className="test-composer"
        onSubmit={(e) => {
          e.preventDefault();
          send(draft);
        }}
      >
        <input
          type="text"
          value={draft}
          maxLength={2000}
          placeholder={strings.creator.testPlaceholder}
          aria-label={strings.creator.testPlaceholder}
          onChange={(e) => {
            setDraft(e.target.value);
          }}
        />
        {busy ? (
          <button type="button" className="button button-stop" onClick={() => void stopReply()}>
            {strings.chat.stop}
          </button>
        ) : (
          <button type="submit" className="button" disabled={!draft.trim() || !ready}>
            {strings.chat.send}
          </button>
        )}
      </form>
      <button
        type="button"
        className="link-button"
        disabled={busy}
        onClick={() => {
          setLines([{ role: "character", content: fields.greeting }]);
          setError(null);
        }}
      >
        {strings.creator.testReset}
      </button>
    </aside>
  );
}
