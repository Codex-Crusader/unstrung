import { useEffect, useRef, useState } from "react";
import {
  chatMemories,
  editMemory,
  forgetKeptMemory,
  keepMemory,
  keptMemories,
  rebuildSummary,
  remember,
  saveSummary,
  setChatUseMemory,
  setMemoryFlags,
  setMemoryMode,
  stopReply,
  toUiError,
  type Kept,
  type Memory,
  type MemoryMode,
  type UiError,
} from "../api";
import { strings } from "../strings";
import { Icon } from "./Icon";
import { ErrorNote } from "./ErrorNote";

/** A fact to add, with the message it belongs to. */
export interface MemoryDraft {
  messageId: number;
  text: string;
}

/** The side panel with what a chat remembers: goal, story so far and facts. */
export function MemoryPanel({
  chatId,
  characterId,
  characterName,
  useKept,
  onUseKept,
  anchorId,
  refreshKey,
  disabled,
  draft,
  onDraftDone,
  onClose,
}: {
  chatId: number;
  characterId: number;
  characterName: string;
  /** The chat uses the kept facts, when the panel opens. */
  useKept: boolean;
  /** The switch changed: the chat screen keeps the value for the next open. */
  onUseKept: (on: boolean) => void;
  /**
   * The message that a fact typed in the panel belongs to: the newest user message.
   * A regenerate replaces the reply, so a fact on it would leave the path.
   */
  anchorId: number | null;
  /** Changes when the list must load again. */
  refreshKey: string;
  disabled: boolean;
  /** A fact from "Remember this" on a message, to check before it is saved. */
  draft: MemoryDraft | null;
  onDraftDone: () => void;
  onClose: () => void;
}) {
  const [memories, setMemories] = useState<Memory[]>([]);
  const [editing, setEditing] = useState<{ id: number; text: string } | null>(null);
  const [adding, setAdding] = useState(draft?.text ?? "");
  // A new "Remember this" fills the field, also while the panel is open.
  const [shownDraft, setShownDraft] = useState(draft);
  if (draft !== shownDraft) {
    setShownDraft(draft);
    if (draft) setAdding(draft.text);
  }
  const [error, setError] = useState<UiError | null>(null);
  const [kept, setKept] = useState<Kept | null>(null);
  // "Rebuild summary": running, or the new text to read before it is saved.
  const [rebuild, setRebuild] = useState<"running" | { throughId: number; text: string } | null>(null);
  const stopped = useRef(false);
  const startRebuild = () => {
    stopped.current = false;
    setRebuild("running");
    setError(null);
    rebuildSummary(chatId)
      .then((r) => {
        setRebuild({ throughId: r.through_id, text: r.summary });
      })
      .catch((e: unknown) => {
        setRebuild(null);
        if (!stopped.current) setError(toUiError(e));
      });
  };

  useEffect(() => {
    let active = true;
    keptMemories(characterId)
      .then((k) => {
        if (active) setKept(k);
      })
      .catch((e: unknown) => {
        if (active) setError(toUiError(e));
      });
    return () => {
      active = false;
    };
  }, [characterId]);

  const runKept = (call: Promise<Kept>) => {
    call
      .then((k) => {
        setKept(k);
        setError(null);
      })
      .catch((e: unknown) => {
        setError(toUiError(e));
      });
  };
  const isKept = (m: Memory) => kept?.memories.some((k) => k.content === m.content.trim()) ?? false;

  useEffect(() => {
    let active = true;
    chatMemories(chatId)
      .then((list) => {
        if (active) setMemories(list);
      })
      .catch((e: unknown) => {
        if (active) setError(toUiError(e));
      });
    return () => {
      active = false;
    };
  }, [chatId, refreshKey]);

  const run = (call: Promise<Memory[]>, done?: () => void) => {
    call
      .then((list) => {
        setMemories(list);
        setError(null);
        done?.();
      })
      .catch((e: unknown) => {
        setError(toUiError(e));
      });
  };

  const source = draft?.messageId ?? anchorId;
  const goal = memories.find((m) => m.kind === "goal");
  const summary = memories.find((m) => m.kind === "summary");
  const facts = memories.filter((m) => m.kind === "fact");

  const entry = (m: Memory) => {
    if (editing?.id === m.id) {
      return (
        <form
          className="memory-edit"
          onSubmit={(e) => {
            e.preventDefault();
            run(editMemory(chatId, m.id, editing.text), () => {
              setEditing(null);
            });
          }}
        >
          <textarea
            value={editing.text}
            rows={m.kind === "summary" ? 6 : 3}
            aria-label={strings.memory.edit}
            onChange={(e) => {
              setEditing({ id: m.id, text: e.target.value });
            }}
          />
          <div className="memory-actions">
            <button
              type="button"
              className="link-button"
              onClick={() => {
                setEditing(null);
              }}
            >
              {strings.memory.cancel}
            </button>
            <button type="submit" className="link-button" disabled={!editing.text.trim()}>
              {strings.memory.save}
            </button>
          </div>
        </form>
      );
    }
    return (
      <>
        <p className={m.is_hidden ? "memory-text memory-hidden" : "memory-text"}>{m.content}</p>
        {(m.is_hidden || m.is_pinned || m.origin === "user" || isKept(m)) && (
          <p className="memory-meta">
            {[
              m.is_hidden && strings.memory.hidden,
              m.is_pinned && strings.memory.pinned,
              m.origin === "user" && strings.memory.yours,
              isKept(m) && strings.memory.kept,
            ]
              .filter(Boolean)
              .join(" · ")}
          </p>
        )}
        <div className="memory-actions">
          <button
            type="button"
            className="link-button"
            disabled={disabled}
            onClick={() => {
              setEditing({ id: m.id, text: m.content });
            }}
          >
            {strings.memory.edit}
          </button>
          {m.kind === "fact" && !m.is_hidden && (
            <button
              type="button"
              className="link-button"
              disabled={disabled}
              onClick={() => {
                run(setMemoryFlags(chatId, m.id, !m.is_pinned, false));
              }}
            >
              {m.is_pinned ? strings.memory.unpin : strings.memory.pin}
            </button>
          )}
          <button
            type="button"
            className="link-button"
            disabled={disabled}
            onClick={() => {
              // A hidden fact is not pinned.
              run(setMemoryFlags(chatId, m.id, m.is_hidden ? m.is_pinned : false, !m.is_hidden));
            }}
          >
            {m.is_hidden ? strings.memory.show : strings.memory.hide}
          </button>
          {m.kind === "fact" && !m.is_hidden && !isKept(m) && (
            <button
              type="button"
              className="link-button"
              disabled={disabled}
              onClick={() => {
                runKept(keepMemory(chatId, m.id));
              }}
            >
              {strings.memory.keep}
            </button>
          )}
        </div>
      </>
    );
  };

  return (
    <aside className="memory-panel" aria-label={strings.memory.title}>
      <div className="chat-list-head">
        <h2>{strings.memory.title}</h2>
        <button type="button" className="link-button" onClick={onClose}>
          <Icon name="close" />
          {strings.memory.close}
        </button>
      </div>
      <p className="memory-intro">{strings.memory.intro}</p>
      {error && <ErrorNote error={error} />}
      {memories.length === 0 && <p className="memory-intro">{strings.memory.empty}</p>}
      {goal && (
        <section className="memory-section">
          <h3>{strings.memory.goal}</h3>
          {entry(goal)}
        </section>
      )}
      {summary && (
        <section className="memory-section">
          <h3>{strings.memory.summary}</h3>
          {rebuild === null && entry(summary)}
          {rebuild === null && (
            <div className="memory-actions">
              <button type="button" className="link-button" disabled={disabled} onClick={startRebuild}>
                {strings.memory.rebuild}
              </button>
            </div>
          )}
          {rebuild === "running" && (
            <p className="memory-intro" role="status">
              {strings.memory.rebuilding}{" "}
              <button
                type="button"
                className="link-button"
                onClick={() => {
                  stopped.current = true;
                  void stopReply().catch(() => undefined);
                }}
              >
                {strings.memory.stop}
              </button>
            </p>
          )}
          {rebuild !== null && rebuild !== "running" && (
            <form
              className="memory-edit"
              onSubmit={(e) => {
                e.preventDefault();
                run(saveSummary(chatId, rebuild.throughId, rebuild.text), () => {
                  setRebuild(null);
                });
              }}
            >
              <p className="memory-intro">{strings.memory.rebuilt}</p>
              <textarea
                value={rebuild.text}
                rows={6}
                aria-label={strings.memory.summary}
                onChange={(e) => {
                  setRebuild({ ...rebuild, text: e.target.value });
                }}
              />
              <div className="memory-actions">
                <button
                  type="button"
                  className="link-button"
                  onClick={() => {
                    setRebuild(null);
                  }}
                >
                  {strings.memory.discard}
                </button>
                <button type="submit" className="link-button" disabled={disabled || !rebuild.text.trim()}>
                  {strings.memory.useRebuilt}
                </button>
              </div>
            </form>
          )}
        </section>
      )}
      {facts.length > 0 && (
        <section className="memory-section">
          <h3>{strings.memory.facts}</h3>
          <ul>
            {facts.map((m) => (
              <li key={m.id} className="memory-fact">
                {entry(m)}
              </li>
            ))}
          </ul>
        </section>
      )}
      {kept && (
        <section className="memory-section">
          <h3>{strings.memory.keptTitle}</h3>
          <p className="memory-intro">{strings.memory.keptIntro(characterName)}</p>
          <label className="check-confirm">
            <input
              type="checkbox"
              checked={useKept}
              disabled={disabled}
              onChange={(e) => {
                const on = e.target.checked;
                setChatUseMemory(chatId, on).then(
                  () => {
                    onUseKept(on);
                  },
                  (err: unknown) => {
                    setError(toUiError(err));
                  },
                );
              }}
            />
            {strings.memory.useHere}
          </label>
          <label className="memory-mode">
            <span>{strings.memory.mode}</span>
            <select
              value={kept.mode}
              disabled={disabled}
              onChange={(e) => {
                runKept(setMemoryMode(characterId, e.target.value as MemoryMode));
              }}
            >
              {strings.memory.modes.map(([v, text]) => (
                <option key={v} value={v}>
                  {text}
                </option>
              ))}
            </select>
          </label>
          {kept.memories.length === 0 ? (
            <p className="memory-intro">{strings.memory.keptEmpty}</p>
          ) : (
            <ul>
              {kept.memories.map((k) => (
                <li key={k.id} className="memory-fact">
                  <p className="memory-text">{k.content}</p>
                  <div className="memory-actions">
                    <button
                      type="button"
                      className="link-button"
                      disabled={disabled}
                      onClick={() => {
                        runKept(forgetKeptMemory(characterId, k.id));
                      }}
                    >
                      {strings.memory.forget}
                    </button>
                  </div>
                </li>
              ))}
            </ul>
          )}
        </section>
      )}
      {source !== null && (
        <form
          className="memory-edit"
          onSubmit={(e) => {
            e.preventDefault();
            run(remember(chatId, source, adding), () => {
              setAdding("");
              onDraftDone();
            });
          }}
        >
          <label>
            <span>{strings.memory.add}</span>
            <textarea
              value={adding}
              rows={3}
              placeholder={strings.memory.addPlaceholder}
              onChange={(e) => {
                setAdding(e.target.value);
              }}
            />
          </label>
          <div className="memory-actions">
            {draft && (
              <button
                type="button"
                className="link-button"
                onClick={() => {
                  setAdding("");
                  onDraftDone();
                }}
              >
                {strings.memory.cancel}
              </button>
            )}
            <button type="submit" className="button" disabled={disabled || !adding.trim()}>
              {strings.memory.save}
            </button>
          </div>
        </form>
      )}
    </aside>
  );
}
