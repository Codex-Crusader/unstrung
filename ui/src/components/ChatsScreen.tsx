import { useEffect, useState } from "react";
import { recentChats, toUiError, type RecentChat, type UiError } from "../api";
import { strings } from "../strings";
import { Avatar } from "./Avatar";
import { Icon } from "./Icon";
import { ErrorNote } from "./ErrorNote";

/** The chats screen: the latest chats with every character, newest first. */
export function ChatsScreen({ onOpenChat }: { onOpenChat: (characterId: number, chatId: number) => void }) {
  const s = strings.chatsPage;
  const [chats, setChats] = useState<RecentChat[] | null>(null);
  const [query, setQuery] = useState("");
  const [error, setError] = useState<UiError | null>(null);

  useEffect(() => {
    recentChats().then(setChats, (e: unknown) => {
      setError(toUiError(e));
    });
  }, []);

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
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
