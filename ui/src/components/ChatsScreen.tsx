import { useEffect, useState } from "react";
import {
  deleteChat,
  deletedChats,
  recentChats,
  restoreChat,
  toUiError,
  type DeletedChat,
  type RecentChat,
  type UiError,
} from "../api";
import { strings } from "../strings";
import { Avatar } from "./Avatar";
import { Icon } from "./Icon";
import { ErrorNote } from "./ErrorNote";

/** The chats screen: the latest chats with every character, newest first. */
export function ChatsScreen({ onOpenChat }: { onOpenChat: (characterId: number, chatId: number) => void }) {
  const s = strings.chatsPage;
  const [chats, setChats] = useState<RecentChat[] | null>(null);
  const [deleted, setDeleted] = useState<DeletedChat[]>([]);
  const [query, setQuery] = useState("");
  const [error, setError] = useState<UiError | null>(null);
  // The chat whose delete asks "are you sure" now.
  const [confirm, setConfirm] = useState<number | null>(null);
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

  const restore = (chatId: number) => {
    setError(null);
    restoreChat(chatId).then(load, (e: unknown) => {
      setError(toUiError(e));
    });
  };

  const wanted = query.trim().toLowerCase();
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
      {chats && chats.length === 0 && (
        <div className="empty-pictures">
          <Icon name="chats" className="setup-icon" />
          <p>{s.emptyTitle}</p>
          <p className="muted">{s.emptyText}</p>
        </div>
      )}
      {shown && shown.length === 0 && wanted && <p className="muted">{s.noMatch(query.trim())}</p>}
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
                  <span className="recent-preview">{r.preview}</span>
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
                )}
              </span>
            </li>
          ))}
        </ul>
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
                    <span className="recent-preview">{d.preview}</span>
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
