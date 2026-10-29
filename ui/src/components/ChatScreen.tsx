import { useCallback, useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import {
  openChat,
  retryReply,
  sendMessage,
  stopReply,
  toUiError,
  type ChatView,
  type EngineStatus,
  type Message,
  type ReplyEvent,
  type UiError,
} from "../api";
import { strings } from "../strings";
import { CharacterAvatar, UserAvatar } from "./Avatar";
import { MessageText } from "./MessageText";

type Reply = { phase: "idle" } | { phase: "thinking" } | { phase: "writing"; text: string };

/** Distance from the bottom, in pixels, that still counts as "at the bottom". */
const STICK_DISTANCE = 80;

export function ChatScreen({ status }: { status: EngineStatus }) {
  const [view, setView] = useState<ChatView | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [reply, setReply] = useState<Reply>({ phase: "idle" });
  const [error, setError] = useState<UiError | null>(null);
  const [showDetail, setShowDetail] = useState(false);
  const [draft, setDraft] = useState("");
  const listRef = useRef<HTMLDivElement>(null);
  const atBottom = useRef(true);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    openChat()
      .then((v) => {
        setView(v);
        setMessages(v.messages);
      })
      .catch((e: unknown) => {
        setError(toUiError(e));
      });
  }, []);

  // Follow new text only when the user is already at the bottom, so reading
  // older messages is not interrupted.
  useLayoutEffect(() => {
    const list = listRef.current;
    if (list && atBottom.current) list.scrollTop = list.scrollHeight;
  }, [messages, reply]);

  const onScroll = () => {
    const list = listRef.current;
    if (list) atBottom.current = list.scrollHeight - list.scrollTop - list.clientHeight < STICK_DISTANCE;
  };

  const handleEvent = useCallback((event: ReplyEvent) => {
    switch (event.kind) {
      case "user_saved":
        setMessages((m) => [...m, event.message]);
        setDraft("");
        break;
      case "text":
        setReply((r) => ({ phase: "writing", text: (r.phase === "writing" ? r.text : "") + event.text }));
        break;
      case "done": {
        const saved = event.message;
        if (saved) setMessages((m) => [...m, saved]);
        setReply({ phase: "idle" });
        break;
      }
      case "failed":
        setReply({ phase: "idle" });
        setError(event.error);
        break;
    }
  }, []);

  const start = (run: () => Promise<null>) => {
    setError(null);
    setShowDetail(false);
    setReply({ phase: "thinking" });
    atBottom.current = true;
    run().catch((e: unknown) => {
      setReply({ phase: "idle" });
      setError(toUiError(e));
    });
  };

  const ready = status.state === "ready";
  const busy = reply.phase !== "idle";
  const name = view?.character.name ?? "";
  const canSend = ready && !busy && view !== null && draft.trim().length > 0;
  const lastIsUser = messages.at(-1)?.role === "user";

  const send = () => {
    if (!canSend) return;
    const chatId = view.chat.id;
    const text = draft;
    start(() => sendMessage(chatId, text, handleEvent));
  };

  const retry = () => {
    if (!view || busy) return;
    const chatId = view.chat.id;
    start(() => retryReply(chatId, handleEvent));
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      send();
    }
  };

  useEffect(() => {
    if (ready && !busy) inputRef.current?.focus();
  }, [ready, busy]);

  if (!view) {
    return (
      <section className="chat chat-empty">
        {error && <p className="chat-error">{strings.chat.loadFailed} {error.detail}</p>}
      </section>
    );
  }

  const character = view.character;
  return (
    <section className="chat">
      <header className="chat-header">
        <CharacterAvatar id={character.id} name={character.name} hasPicture={character.avatar_path !== null} />
        <div className="chat-title">
          <h1>{character.name}</h1>
          <p>{character.tagline}</p>
        </div>
      </header>

      <div className="messages" ref={listRef} onScroll={onScroll} role="log" aria-label={strings.chat.messages}>
        {messages.map((m) => (
          <article key={m.id} className={`message message-${m.role}`}>
            {m.role === "user" ? (
              <UserAvatar label={strings.chat.you} />
            ) : (
              <CharacterAvatar id={character.id} name={character.name} hasPicture={character.avatar_path !== null} />
            )}
            <div className="bubble">
              <span className="speaker">{m.role === "user" ? strings.chat.you : character.name}</span>
              <MessageText text={m.content} />
            </div>
          </article>
        ))}
        {reply.phase !== "idle" && (
          <article className="message message-character" aria-busy="true">
            <CharacterAvatar id={character.id} name={character.name} hasPicture={character.avatar_path !== null} />
            <div className="bubble">
              <span className="speaker">{character.name}</span>
              {reply.phase === "thinking" ? (
                <span className="typing" role="img" aria-label={strings.chat.thinking(name)}>
                  <span />
                  <span />
                  <span />
                </span>
              ) : (
                <MessageText text={reply.text} />
              )}
            </div>
          </article>
        )}
        {error && (
          <div className="chat-error" role="alert">
            <span>{error.message}</span>
            {lastIsUser && (
              <button type="button" className="link-button" onClick={retry} disabled={!ready}>
                {strings.chat.tryAgain}
              </button>
            )}
            <button
              type="button"
              className="link-button"
              onClick={() => {
                setShowDetail((v) => !v);
              }}
            >
              {showDetail ? strings.status.hideDetails : strings.status.details}
            </button>
            {showDetail && <code className="error-detail">{error.detail}</code>}
          </div>
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
                : strings.chat.notReady(name)
          }
          aria-label={strings.chat.placeholder(name)}
          disabled={!ready}
          onChange={(e) => {
            setDraft(e.target.value);
          }}
          onKeyDown={onKeyDown}
        />
        {busy ? (
          <button type="button" className="button button-stop" onClick={() => void stopReply()}>
            {strings.chat.stop}
          </button>
        ) : (
          <button type="submit" className="button" disabled={!canSend}>
            {strings.chat.send}
          </button>
        )}
      </form>
    </section>
  );
}
