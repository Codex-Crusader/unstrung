import { memo, useLayoutEffect, useMemo, useRef, useState, type ComponentProps, type ReactNode } from "react";
import type { Character, Painting, Persona, Picture, RegenMode, ViewMessage } from "../api";
import { FadeImage, PaintingCard } from "./PictureViewer";
import { strings } from "../strings";
import { Avatar } from "./Avatar";
import { MessageText } from "./MessageText";
import { Icon, type IconName } from "./Icon";

export interface MessageActions {
  showVersion: (messageId: number) => void;
  regenerate: (messageId: number, mode: RegenMode) => void;
  continueReply: () => void;
  edit: (message: ViewMessage, text: string) => void;
  rewind: (messageId: number) => void;
  branch: (messageId: number) => void;
  remember: (message: ViewMessage) => void;
  remove: (messageId: number) => void;
  picture: (messageId: number) => void;
}


/**
 * One message. While a reply streams, the chat screen draws again for every
 * piece of text. This wrapper gives the memoized card actions that keep their
 * identity and call the newest ones, so the messages above stay as they are.
 */
export function MessageItem(props: ComponentProps<typeof MessageCard>) {
  const latest = useRef(props.actions);
  useLayoutEffect(() => {
    latest.current = props.actions;
  });
  const actions = useMemo<MessageActions>(
    () => ({
      showVersion: (id) => {
        latest.current.showVersion(id);
      },
      regenerate: (id, mode) => {
        latest.current.regenerate(id, mode);
      },
      continueReply: () => {
        latest.current.continueReply();
      },
      edit: (message, text) => {
        latest.current.edit(message, text);
      },
      rewind: (id) => {
        latest.current.rewind(id);
      },
      branch: (id) => {
        latest.current.branch(id);
      },
      remember: (message) => {
        latest.current.remember(message);
      },
      remove: (id) => {
        latest.current.remove(id);
      },
      picture: (id) => {
        latest.current.picture(id);
      },
    }),
    [],
  );
  return <MessageCard {...props} actions={actions} />;
}

/** Every prop must keep its identity between draws, or the memo does nothing. */
const MessageCard = memo(function MessageCard({
  message,
  character,
  persona,
  isLast,
  locked,
  actions,
  pictures,
  painting,
  oldPictures,
  onOpenPicture,
}: {
  message: ViewMessage;
  character: Character;
  /** The persona that wrote a user message, if it still exists. */
  persona: Persona | null;
  isLast: boolean;
  /** True while a reply is written or the engine is not ready. */
  locked: boolean;
  actions: MessageActions;
  /** The pictures made for this message. */
  pictures: Picture[];
  /** A picture being made for this message. */
  painting: Painting | null;
  /** The pictures that were there when the chat opened. The others develop into view. */
  oldPictures: ReadonlySet<number>;
  onOpenPicture: (id: number) => void;
}) {
  const [editing, setEditing] = useState<string | null>(null);
  const menu = useRef<HTMLDetailsElement>(null);

  if (message.role === "system") {
    return <p className="system-note">{message.content}</p>;
  }

  const fromCharacter = message.role === "character";
  const speaker = fromCharacter ? character.name : (persona?.name ?? strings.chat.you);
  const at = message.siblings.indexOf(message.id);
  const count = message.siblings.length;

  const act = (run: () => void) => () => {
    menu.current?.removeAttribute("open");
    run();
  };
  const item = (icon: IconName, label: string, run: () => void, extra?: string): ReactNode => (
    <li key={label}>
      <button type="button" className={extra ? `menu-item ${extra}` : "menu-item"} onClick={act(run)}>
        <Icon name={icon} />
        {label}
      </button>
    </li>
  );

  return (
    <article id={`message-${String(message.id)}`} className={`message message-${message.role}`}>
      {fromCharacter ? (
        <Avatar kind="character" owner={character} />
      ) : (
        <Avatar kind="persona" owner={persona} label={speaker} />
      )}
      <div className="bubble">
        <span className="speaker">
          {message.role === "ooc" ? `${speaker} · ${strings.chat.oocLabel}` : speaker}
        </span>
        {editing === null ? (
          <>
            <MessageText text={message.content} />
            {(pictures.length > 0 || painting) && (
              <div className="message-pictures">
                {pictures.map((p) => (
                  <button
                    key={p.id}
                    type="button"
                    className="message-picture"
                    aria-label={strings.chat.openPicture}
                    onClick={() => {
                      onOpenPicture(p.id);
                    }}
                  >
                    <FadeImage picture={p} alt={strings.chat.picture} develop={!oldPictures.has(p.id)} />
                  </button>
                ))}
                {painting && <PaintingCard painting={painting} />}
              </div>
            )}
          </>
        ) : (
          <form
            className="edit-form"
            onSubmit={(e) => {
              e.preventDefault();
              if (editing.trim()) {
                actions.edit(message, editing);
                setEditing(null);
              }
            }}
          >
            <textarea
              value={editing}
              rows={Math.min(10, Math.max(3, editing.split("\n").length + 1))}
              maxLength={8000}
              aria-label={strings.message.edit}
              onChange={(e) => {
                setEditing(e.target.value);
              }}
            />
            <div className="edit-actions">
              <button
                type="button"
                className="button button-quiet"
                onClick={() => {
                  setEditing(null);
                }}
              >
                {strings.message.cancel}
              </button>
              <button type="submit" className="button" disabled={locked || !editing.trim()}>
                {strings.message.save}
              </button>
            </div>
          </form>
        )}
        <div className="message-tools">
          {count > 1 && (
            <span className="versions">
              <button
                type="button"
                className="icon-button"
                aria-label={strings.message.previous}
                disabled={locked || at <= 0}
                onClick={() => {
                  const id = message.siblings[at - 1];
                  if (id !== undefined) actions.showVersion(id);
                }}
              >
                ‹
              </button>
              <span>{strings.message.version(at + 1, count)}</span>
              <button
                type="button"
                className="icon-button"
                aria-label={strings.message.next}
                disabled={locked || at >= count - 1}
                onClick={() => {
                  const id = message.siblings[at + 1];
                  if (id !== undefined) actions.showVersion(id);
                }}
              >
                ›
              </button>
            </span>
          )}
          {!locked && editing === null && (
            <details className="menu" ref={menu}>
              <summary aria-label={strings.message.menu}>
                <Icon name="more" />
              </summary>
              <ul className="menu-list">
                {fromCharacter && [
                  item("regenerate", strings.message.regenerate, () => {
                    actions.regenerate(message.id, "normal");
                  }),
                  item("shorter", strings.message.shorter, () => {
                    actions.regenerate(message.id, "shorter");
                  }),
                  item("longer", strings.message.longer, () => {
                    actions.regenerate(message.id, "longer");
                  }),
                ]}
                {fromCharacter && message.parent_id !== null && [
                  item("inCharacter", strings.message.inCharacter, () => {
                    actions.regenerate(message.id, "in_character");
                  }),
                  item("close", strings.message.notInCharacter, () => {
                    actions.regenerate(message.id, "not_in_character");
                  }),
                ]}
                {fromCharacter && isLast && item("continue", strings.message.continue, actions.continueReply)}
                {item("edit", strings.message.edit, () => {
                  setEditing(message.content);
                })}
                {!isLast &&
                  item("rewind", strings.message.rewind, () => {
                    actions.rewind(message.id);
                  })}
                {item("branch", strings.message.branch, () => {
                  actions.branch(message.id);
                })}
                {item("remember", strings.message.remember, () => {
                  actions.remember(message);
                })}
                {message.role !== "ooc" &&
                  item("picture", strings.chat.pictureThis, () => {
                    actions.picture(message.id);
                  })}
                {message.parent_id !== null &&
                  item(
                    "remove",
                    strings.message.delete,
                    () => {
                      actions.remove(message.id);
                    },
                    "danger",
                  )}
              </ul>
            </details>
          )}
        </div>
      </div>
    </article>
  );
});
