import { useCallback, useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import {
  branchChat,
  continueReply,
  deleteMessage,
  editMessage,
  editReply,
  newChat,
  onMemoryChanged,
  openCharacter,
  openChat,
  regenerate,
  retryReply,
  rewind,
  sendMessage,
  setChatPersona,
  setChatStyle,
  setChatModel,
  modelsPage,
  setChatTone,
  showBranch,
  stopReply,
  toUiError,
  undoDelete,
  type ChatView,
  type DeleteUndo,
  type EngineStatus,
  type Persona,
  type ReplyEvent,
  type ReplyHandler,
  type UiError,
  type ViewMessage,
  chatPictures,
  onPainting,
  paintPicture,
  picturesPage,
  type Painting,
  type Picture,
  type Staged,
} from "../api";
import { strings } from "../strings";
import { Avatar } from "./Avatar";
import { ChatHeader } from "./ChatHeader";
import { ChatList } from "./ChatList";
import { MemoryPanel, type MemoryDraft } from "./MemoryPanel";
import { MessageItem, type MessageActions } from "./MessageItem";
import { MessageText } from "./MessageText";
import { Icon } from "./Icon";
import { ErrorNote } from "./ErrorNote";
import { PictureViewer, withCharacter } from "./PictureViewer";

type Reply = { phase: "idle" } | { phase: "thinking" } | { phase: "writing"; text: string };

/** Distance from the bottom, in pixels, that still counts as "at the bottom". */
const STICK_DISTANCE = 80;

/** `replyAfter` value for a new greeting: no earlier message stays in view. Message IDs start at 1. */
const BEFORE_FIRST = 0;

export function ChatScreen({
  status,
  characterId,
  chatId: startChat,
  personas,
  onBack,
  onUseAsAvatar,
}: {
  status: EngineStatus;
  characterId: number;
  /** A chat to open. Without it, the character's latest chat opens. */
  chatId?: number | undefined;
  personas: Persona[];
  onBack: () => void;
  onUseAsAvatar: (characterId: number, staged: Staged) => void;
}) {
  const [view, setView] = useState<ChatView | null>(null);
  const [pending, setPending] = useState<ViewMessage | null>(null);
  const [reply, setReply] = useState<Reply>({ phase: "idle" });
  // The message under the reply that streams now. Messages after it are hidden.
  const [replyAfter, setReplyAfter] = useState<number | null>(null);
  const [error, setError] = useState<UiError | null>(null);
  const [hint, setHint] = useState<string | null>(null);
  const [undo, setUndo] = useState<DeleteUndo | null>(null);
  const [listOpen, setListOpen] = useState(false);
  const [memoryOpen, setMemoryOpen] = useState(false);
  const [memoryDraft, setMemoryDraft] = useState<MemoryDraft | null>(null);
  // Counts the memory passes of this chat, so the panel loads again.
  const [memoryTick, setMemoryTick] = useState(0);
  const [draft, setDraft] = useState("");
  const [pictures, setPictures] = useState<Picture[]>([]);
  // The pictures the chat had when it opened. Later ones develop into view.
  const [oldPictures, setOldPictures] = useState<Set<number>>(new Set());
  const [openPicture, setOpenPicture] = useState<number | null>(null);
  // Set while a picture is made, from the click until the command returns.
  const [painting, setPainting] = useState<Painting | null>(null);
  const [picturesReady, setPicturesReady] = useState(false);
  const listRef = useRef<HTMLDivElement>(null);
  const atBottom = useRef(true);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const [models, setModels] = useState<{ id: string; name: string }[]>([]);
  const [defaultModel, setDefaultModel] = useState("");

  // A memory pass ends after the reply, while the user reads it.
  const openChatId = view?.chat.id;
  useEffect(() => {
    if (openChatId === undefined) return;
    let active = true;
    const unlisten = onMemoryChanged((id) => {
      if (active && id === openChatId) setMemoryTick((t) => t + 1);
    });
    return () => {
      active = false;
      void unlisten.then((stop) => {
        stop();
      });
    };
  }, [openChatId]);

  const openChatIdForPictures = view?.chat.id;
  // The listener below lives as long as the screen, so it reads the open chat here.
  const shownChat = useRef<number | null>(null);
  useEffect(() => {
    shownChat.current = openChatIdForPictures ?? null;
    if (openChatIdForPictures === undefined) return;
    let active = true;
    chatPictures(openChatIdForPictures)
      .then((p) => {
        if (!active) return;
        setPictures(p);
        setOldPictures(new Set(p.map((x) => x.id)));
      })
      .catch(() => {
        // Without the list, the chat shows no pictures.
      });
    return () => {
      active = false;
    };
  }, [openChatIdForPictures]);

  useEffect(() => {
    picturesPage()
      .then((p) => {
        setPicturesReady(p.engine_installed && p.models.some((m) => m.selected && m.installed));
      })
      .catch(() => {
        // Pictures stay off.
      });
    // Events, not the command, drive the progress: after a switch to another
    // screen and back, a running picture still shows and still lands here.
    const off = onPainting((p) => {
      const here = shownChat.current;
      if (p) {
        if (p.chat_id === here) setPainting(p);
        return;
      }
      setPainting(null);
      if (here !== null) {
        chatPictures(here).then(setPictures, () => {
          // The list stays as it is.
        });
      }
    });
    return () => {
      void off.then((f) => {
        f();
      });
    };
  }, []);

  // The model picker in the header lists the installed models.
  useEffect(() => {
    let active = true;
    if (status.state !== "ready") return;
    modelsPage()
      .then((p) => {
        if (!active) return;
        setModels(p.models.filter((m) => m.installed).map((m) => ({ id: m.id, name: m.name })));
        setDefaultModel(p.default_model);
      })
      .catch(() => {
        // Without the list, the header has no model picker.
      });
    return () => {
      active = false;
    };
  }, [status.state]);

  useEffect(() => {
    let active = true;
    (startChat === undefined ? openCharacter(characterId) : openChat(startChat))
      .then((v) => {
        if (active) setView(v);
      })
      .catch((e: unknown) => {
        if (active) setError(toUiError(e));
      });
    return () => {
      active = false;
    };
  }, [characterId, startChat]);

  // Follow new text only when the user is already at the bottom, so reading
  // older messages is not interrupted.
  useLayoutEffect(() => {
    const list = listRef.current;
    if (list && atBottom.current) list.scrollTop = list.scrollHeight;
  }, [view, pending, reply]);

  const onScroll = () => {
    const list = listRef.current;
    if (list) atBottom.current = list.scrollHeight - list.scrollTop - list.clientHeight < STICK_DISTANCE;
  };

  const handleEvent = useCallback((event: ReplyEvent) => {
    switch (event.kind) {
      case "user_saved":
        setPending({ ...event.message, siblings: [event.message.id] });
        setDraft("");
        break;
      case "text":
        setReply((r) => ({ phase: "writing", text: (r.phase === "writing" ? r.text : "") + event.text }));
        break;
      case "restart":
        setReply({ phase: "thinking" });
        break;
      case "done":
        setView(event.view);
        setPending(null);
        setReply({ phase: "idle" });
        setReplyAfter(null);
        setHint(event.hint);
        break;
      case "failed":
        setReply({ phase: "idle" });
        setReplyAfter(null);
        setError(event.error);
        break;
    }
  }, []);

  const ready = status.state === "ready";
  const busy = reply.phase !== "idle";
  const locked = busy || painting !== null || !ready;

  /** Starts a streamed reply. `after` is the message the reply answers. */
  const start = (after: number | null, run: (handler: ReplyHandler) => Promise<null>) => {
    setError(null);
    setHint(null);
    setUndo(null);
    setReply({ phase: "thinking" });
    setReplyAfter(after);
    atBottom.current = true;
    run((event) => {
      // A failed reply can still have saved the user message, so reload the chat.
      if (event.kind === "failed" && view) {
        const chatId = view.chat.id;
        openChat(chatId)
          .then(setView)
          .catch(() => {
            // The error below already tells the user that the reply failed.
          });
        setPending(null);
      }
      handleEvent(event);
    }).catch((e: unknown) => {
      setReply({ phase: "idle" });
      setReplyAfter(null);
      setError(toUiError(e));
    });
  };

  /** Runs a quick change that returns the new view. */
  const change = (run: () => Promise<ChatView>, after?: () => void) => {
    setError(null);
    setHint(null);
    run()
      .then((v) => {
        setView(v);
        after?.();
      })
      .catch((e: unknown) => {
        setError(toUiError(e));
      });
  };

  if (!view) {
    return (
      <section className="chat chat-empty">
        {error && <ErrorNote error={error} lead={strings.chat.loadFailed} />}
      </section>
    );
  }

  const chatId = view.chat.id;
  const character = view.character;
  const name = character.name;
  const personaOf = (id: number | null) => personas.find((p) => p.id === id) ?? null;
  const canSend = !locked && draft.trim().length > 0;
  const last = view.messages.at(-1);
  const lastIsUser = last !== undefined && last.role !== "character";

  // While a reply streams, hide the messages it replaces.
  let shown = view.messages;
  if (replyAfter === BEFORE_FIRST) {
    shown = [];
  } else if (replyAfter !== null) {
    const at = shown.findIndex((m) => m.id === replyAfter);
    if (at >= 0) shown = shown.slice(0, at + 1);
  }
  if (pending) {
    // An edit puts the new message in place of the old one.
    const at = pending.parent_id === null ? -1 : shown.findIndex((m) => m.id === pending.parent_id);
    shown = [...shown.slice(0, at + 1), pending];
  }

  /** Makes a picture of a message, or of the latest one when `messageId` is null. */
  const paint = (messageId: number | null, wish: string) => {
    setError(null);
    setHint(null);
    if (!picturesReady) {
      setHint(strings.chat.picturesOff);
      return;
    }
    // The picture is still made: the note says why faces can change.
    if (character.appearance_tags.trim() === "") setHint(strings.chat.noLooks(name));
    setPainting({ chat_id: chatId, message_id: messageId, phase: "scene", step: 0, total: 0 });
    paintPicture(chatId, messageId, wish)
      .then((p) => {
        if (p) setPictures((all) => (all.some((x) => x.id === p.id) ? all : [...all, p]));
      })
      .catch((e: unknown) => {
        setError(toUiError(e));
      })
      .finally(() => {
        setPainting(null);
      });
  };

  const send = () => {
    if (!canSend) return;
    const text = draft;
    const imagine = /^\/imagine\b\s*/i.exec(text);
    if (imagine) {
      setDraft("");
      paint(null, text.slice(imagine[0].length));
      return;
    }
    start(last?.id ?? null, (h) => sendMessage(chatId, text, h));
  };

  const actions: MessageActions = {
    showVersion: (id) => {
      change(() => showBranch(chatId, id));
    },
    regenerate: (id, mode) => {
      const message = view.messages.find((m) => m.id === id);
      // A new greeting replaces the whole chat view while it streams.
      start(message?.parent_id ?? BEFORE_FIRST, (h) => regenerate(chatId, id, mode, h));
    },
    continueReply: () => {
      start(null, (h) => continueReply(chatId, h));
    },
    edit: (message, text) => {
      if (message.role === "character") {
        change(() => editReply(chatId, message.id, text));
      } else {
        start(message.parent_id, (h) => editMessage(chatId, message.id, text, h));
      }
    },
    rewind: (id) => {
      change(
        () => rewind(chatId, id),
        () => {
          setHint(strings.message.rewound);
        },
      );
    },
    branch: (id) => {
      change(() => branchChat(chatId, id));
    },
    remember: (message) => {
      setListOpen(false);
      setMemoryOpen(true);
      setMemoryDraft({ messageId: message.id, text: message.content.split(/\s+/).slice(0, 40).join(" ") });
    },
    picture: (id) => {
      paint(id, "");
    },
    remove: (id) => {
      setError(null);
      deleteMessage(chatId, id)
        .then((result) => {
          setView(result.view);
          setUndo(result.undo);
        })
        .catch((e: unknown) => {
          setError(toUiError(e));
        });
    },
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      send();
    }
  };

  const streaming = busy && (
    <article className="message message-character" aria-busy="true">
      <Avatar kind="character" owner={character} />
      <div className="bubble">
        <span className="speaker">{name}</span>
        {reply.phase === "writing" ? (
          <MessageText text={reply.text} />
        ) : (
          <span className="typing" role="img" aria-label={strings.chat.thinking(name)}>
            <span />
            <span />
            <span />
          </span>
        )}
      </div>
    </article>
  );

  return (
    <div className="chat-layout">
      <section className="chat">
        <ChatHeader
          view={view}
          personas={personas}
          locked={locked}
          onBack={onBack}
          onPersona={(id) => {
            change(() => setChatPersona(chatId, id));
          }}
          onTone={(tone, custom) => {
            change(() => setChatTone(chatId, tone, custom));
          }}
          onStyle={(style, custom) => {
            change(() => setChatStyle(chatId, style, custom));
          }}
          onModel={(model) => {
            change(() => setChatModel(chatId, model));
          }}
          models={models}
          defaultModel={defaultModel}
          onToggleChats={() => {
            setMemoryOpen(false);
            setListOpen((v) => !v);
          }}
          chatsOpen={listOpen}
          memoryOpen={memoryOpen}
          onToggleMemory={() => {
            setListOpen(false);
            setMemoryOpen((v) => !v);
          }}
        />

        {view.model_removed && <p className="chat-notice">{strings.header.modelRemoved(view.model_removed)}</p>}
        <div
          className="messages"
          ref={listRef}
          onScroll={onScroll}
          role="log"
          aria-label={strings.chat.messages}
          // While a reply streams, a screen reader waits and reads it once.
          aria-busy={busy}
        >
          {shown.map((m, i) => (
            <MessageItem
              key={m.id}
              message={m}
              character={character}
              persona={personaOf(m.persona_id)}
              isLast={i === shown.length - 1}
              locked={locked}
              actions={actions}
              pictures={pictures.filter((p) => p.message_id === m.id)}
              painting={painting && (painting.message_id ?? last?.id) === m.id ? painting : null}
              isNew={(id) => !oldPictures.has(id)}
              onOpenPicture={setOpenPicture}
            />
          ))}
          {streaming}
          {!busy && view.branches.length > 0 && (
            <p className="system-note">
              {strings.chat.hiddenBranch(view.branches.length)}{" "}
              <button
                type="button"
                className="link-button"
                disabled={locked}
                onClick={() => {
                  const newest = view.branches.at(-1);
                  if (newest !== undefined) actions.showVersion(newest);
                }}
              >
                {strings.chat.showHidden}
              </button>
            </p>
          )}
          {hint && !busy && (
            <p className="hint" role="status">
              {hint}
            </p>
          )}
          {undo && (
            <p className="toast" role="status">
              {strings.chat.deleted}{" "}
              <button
                type="button"
                className="link-button"
                disabled={locked}
                onClick={() => {
                  const token = undo;
                  setUndo(null);
                  change(() => undoDelete(chatId, token));
                }}
              >
                {strings.chat.undo}
              </button>
              <button
                type="button"
                className="link-button"
                onClick={() => {
                  setUndo(null);
                }}
              >
                {strings.chat.dismiss}
              </button>
            </p>
          )}
          {error && (
            <ErrorNote error={error}>
              {lastIsUser && !busy && (
                <button
                  type="button"
                  className="link-button"
                  onClick={() => {
                    start(last.id, (h) => retryReply(chatId, h));
                  }}
                  disabled={!ready}
                >
                  {strings.chat.tryAgain}
                </button>
              )}
            </ErrorNote>
          )}
        </div>

        <form
          className="composer"
          onSubmit={(e) => {
            e.preventDefault();
            send();
          }}
        >
          <textarea
            ref={inputRef}
            value={draft}
            rows={2}
            maxLength={8000}
            placeholder={
              ready
                ? strings.chat.placeholder(name)
                : status.state === "error"
                  ? status.message
                  : status.state === "no_model"
                    ? strings.status.noModel(status.model)
                    : status.state === "not_installed"
                      ? strings.status.notInstalled
                      : strings.chat.notReady(name)
            }
            title={`${strings.chat.oocHint} ${strings.chat.imagineHint}`}
            aria-label={strings.chat.placeholder(name)}
            disabled={!ready}
            onChange={(e) => {
              setDraft(e.target.value);
            }}
            onKeyDown={onKeyDown}
          />
          {busy || painting ? (
            <button type="button" className="button button-stop" onClick={() => void stopReply()}>
              <Icon name="stop" />
              {strings.chat.stop}
            </button>
          ) : (
            <button type="submit" className="button" disabled={!canSend}>
              <Icon name="send" />
              {strings.chat.send}
            </button>
          )}
        </form>
      </section>
      {openPicture !== null && (
        <PictureViewer
          pictures={pictures.map((p) => withCharacter(p, character.id, name))}
          openId={openPicture}
          onOpen={setOpenPicture}
          onClose={() => {
            setOpenPicture(null);
          }}
          onMade={(p) => {
            setPictures((all) => (all.some((x) => x.id === p.id) ? all : [...all, p]));
            setOpenPicture(p.id);
          }}
          onDeleted={(id) => {
            const at = pictures.findIndex((p) => p.id === id);
            const next = pictures[at + 1] ?? pictures[at - 1];
            setPictures((all) => all.filter((p) => p.id !== id));
            setOpenPicture(next ? next.id : null);
          }}
          onUseAsAvatar={(_, staged) => {
            onUseAsAvatar(character.id, staged);
          }}
        />
      )}
      {listOpen && (
        <ChatList
          characterId={character.id}
          currentChatId={chatId}
          refreshKey={`${String(chatId)}-${String(view.chat.updated_at)}`}
          disabled={locked}
          onOpen={(id) => {
            change(() => openChat(id));
          }}
          onNew={(useMemory) => {
            change(() => newChat(character.id, view.chat.persona_id, useMemory));
          }}
          onDeleted={(id) => {
            if (id === chatId) change(() => openCharacter(character.id));
          }}
          onClose={() => {
            setListOpen(false);
          }}
        />
      )}
      {memoryOpen && (
        <MemoryPanel
          key={chatId}
          chatId={chatId}
          characterId={character.id}
          characterName={character.name}
          useKept={view.chat.use_character_memory}
          onUseKept={(on) => {
            setView((v) => (v ? { ...v, chat: { ...v.chat, use_character_memory: on } } : v));
          }}
          anchorId={view.messages.findLast((m) => m.role === "user")?.id ?? view.chat.head_message_id}
          refreshKey={`${String(view.chat.head_message_id)}-${String(memoryTick)}`}
          disabled={locked}
          draft={memoryDraft}
          onDraftDone={() => {
            setMemoryDraft(null);
          }}
          onClose={() => {
            setMemoryOpen(false);
            setMemoryDraft(null);
          }}
        />
      )}
    </div>
  );
}
