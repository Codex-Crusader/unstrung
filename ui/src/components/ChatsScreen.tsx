import { useEffect, useState } from "react";
import {
  deleteChat,
  deletedChats,
  plainPreview,
  recentChats,
  saveChatText,
  restoreChat,
  searchChats,
  searchTooShort,
  toUiError,
  type DeletedChat,
  type RecentChat,
  type SearchHit,
  type UiError,
} from "../api";
import { strings } from "../strings";
import { Avatar } from "./Avatar";
import { Icon } from "./Icon";
import { ErrorNote } from "./ErrorNote";
import { Loading } from "./Loading";

/** The chats screen: the latest chats with every character, newest first. */
export function ChatsScreen({
  onOpenChat,
}: {
  /** With a message: the chat opens at that message. */
  onOpenChat: (characterId: number, chatId: number, messageId?: number) => void;
}) {
  const s = strings.chatsPage;
  const [chats, setChats] = useState<RecentChat[] | null>(null);
  const [deleted, setDeleted] = useState<DeletedChat[]>([]);
  const [query, setQuery] = useState("");
  const [error, setError] = useState<UiError | null>(null);
  // The chat whose delete asks "are you sure" now.
  const [confirm, setConfirm] = useState<number | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  // The last search answer, with the text that it answers.
  const [found, setFound] = useState<{ text: string; hits: SearchHit[] } | null>(null);
  const c = strings.chats;

  const remove = (chatId: number) => {
    setError(null);
    deleteChat(chatId).then(
      () => {
        setConfirm(null);
        setChats((all) => all?.filter((x) => x.chat_id !== chatId) ?? null);
        load();
      },
      (e: unknown) => {
        setError(toUiError(e));
      },
    );
  };

  const load = () => {
    recentChats().then(setChats, (e: unknown) => {
      setError(toUiError(e));
    });
    // The list is a help: without it the screen still works.
    deletedChats().then(setDeleted, () => undefined);
  };
  useEffect(load, []);

  const saveText = (chatId: number) => {
    setError(null);
    setSaved(null);
    saveChatText(chatId).then(
      (file) => {
        if (file !== null) setSaved(s.savedText(file));
      },
      (e: unknown) => {
        setError(toUiError(e));
      },
    );
  };

  const restore = (chatId: number) => {
    setError(null);
    restoreChat(chatId).then(load, (e: unknown) => {
      setError(toUiError(e));
    });
  };

  const text = query.trim();
  const short = searchTooShort(text);
  // null while the search for this text waits or runs.
  const hits = short ? [] : found?.text === text ? found.hits : null;
  useEffect(() => {
    if (short) return;
    let active = true;
    // Waits for a pause in typing, so not every key runs a search.
    const timer = window.setTimeout(() => {
      searchChats(text).then(
        (list) => {
          if (active) setFound({ text, hits: list });
        },
        () => {
          if (active) setFound({ text, hits: [] });
        },
      );
    }, 300);
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [text, short]);

  const wanted = text.toLowerCase();
  const shown = wanted
    ? chats?.filter((c) => `${c.character_name} ${c.preview}`.toLowerCase().includes(wanted))
    : chats;

  return (
    <section className="page chats">
      <div className="home-head">
        <h1>{s.title}</h1>
        {chats && chats.length > 0 && (
          <label className="home-search">
            <Icon name="search" />
            <span className="sr-only">{s.search}</span>
            <input
              type="search"
              value={query}
              placeholder={s.search}
              onChange={(e) => {
                setQuery(e.target.value);
              }}
            />
          </label>
        )}
      </div>
      {error && <ErrorNote error={error} />}
      {saved && (
        <p className="field-hint" role="status">
          {saved}
        </p>
      )}
      {chats === null && !error && <Loading />}
      {chats && chats.length === 0 && (
        <div className="empty-pictures">
          <Icon name="chats" className="setup-icon" />
          <p>{s.emptyTitle}</p>
          <p className="muted">{s.emptyText}</p>
        </div>
      )}
      {shown && shown.length === 0 && wanted && hits?.length === 0 && <p className="muted">{s.noMatch(text)}</p>}
      {shown && shown.length > 0 && (
        <ul className="chat-rows">
          {shown.map((r) => (
            <li key={r.chat_id}>
              <button
                type="button"
                className="recent-chat"
                onClick={() => {
                  onOpenChat(r.character_id, r.chat_id);
                }}
              >
                <Avatar
                  kind="character"
                  owner={{ id: r.character_id, name: r.character_name, avatar_path: r.avatar_path }}
                />
                <span className="recent-text">
                  <span className="recent-name">{r.character_name}</span>
                  <span className="recent-preview">{plainPreview(r.preview)}</span>
                </span>
                <span className="chat-when field-hint">{strings.home.when(r.updated_at)}</span>
              </button>
              <span className="chat-delete">
                {confirm === r.chat_id ? (
                  <>
                    <span>{c.confirmDelete}</span>
                    <button
                      type="button"
                      className="button danger-fill"
                      onClick={() => {
                        remove(r.chat_id);
                      }}
                    >
                      {c.yes}
                    </button>
                    <button
                      type="button"
                      className="link-button"
                      onClick={() => {
                        setConfirm(null);
                      }}
                    >
                      {c.no}
                    </button>
                  </>
                ) : (
                  <>
                    <button
                      type="button"
                      className="icon-button"
                      aria-label={`${s.saveText}: ${r.character_name}`}
                      title={s.saveText}
                      onClick={() => {
                        saveText(r.chat_id);
                      }}
                    >
                      <Icon name="download" />
                    </button>
                    <button
                      type="button"
                      className="icon-button"
                      aria-label={`${c.delete}: ${r.character_name}`}
                      title={c.delete}
                      onClick={() => {
                        setConfirm(r.chat_id);
                      }}
                    >
                      <Icon name="remove" />
                    </button>
                  </>
                )}
              </span>
            </li>
          ))}
        </ul>
      )}
      {hits === null && <p className="field-hint">{s.searching}</p>}
      {hits && hits.length > 0 && (
        <section className="search-hits" aria-label={s.inMessages(hits.length)}>
          <h2>{s.inMessages(hits.length)}</h2>
          <ul className="chat-rows">
            {hits.map((h) => (
              <li key={h.message_id}>
                <button
                  type="button"
                  className="recent-chat"
                  onClick={() => {
                    onOpenChat(h.character_id, h.chat_id, h.message_id);
                  }}
                >
                  <Avatar
                    kind="character"
                    owner={{ id: h.character_id, name: h.character_name, avatar_path: h.avatar_path }}
                  />
                  <span className="recent-text">
                    <span className="recent-name">{h.character_name}</span>
                    <span className="recent-preview">
                      {h.before}
                      <mark>{h.hit}</mark>
                      {h.after}
                    </span>
                  </span>
                  <span className="chat-when field-hint">{strings.home.when(h.created_at)}</span>
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}
      {deleted.length > 0 && (
        <details className="more deleted-chats">
          <summary>{s.deletedTitle(deleted.length)}</summary>
          <p className="field-hint">{s.deletedHint}</p>
          <ul className="chat-rows">
            {deleted.map((d) => (
              <li key={d.chat_id}>
                <div className="recent-chat">
                  <Avatar
                    kind="character"
                    owner={{ id: d.character_id, name: d.character_name, avatar_path: d.avatar_path }}
                  />
                  <span className="recent-text">
                    <span className="recent-name">{d.character_name}</span>
                    <span className="recent-preview">{plainPreview(d.preview)}</span>
                  </span>
                  <span className="chat-when field-hint">{s.deletedWhen(d.deleted_at)}</span>
                </div>
                <span className="chat-delete">
                  <button
                    type="button"
                    className="button button-quiet"
                    aria-label={`${s.restore}: ${d.character_name}`}
                    onClick={() => {
                      restore(d.chat_id);
                    }}
                  >
                    {s.restore}
                  </button>
                </span>
              </li>
            ))}
          </ul>
        </details>
      )}
    </section>
  );
}
