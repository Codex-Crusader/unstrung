import { useEffect, useState } from "react";
import { deleteChat, listChats, toUiError, type ChatSummary, type UiError } from "../api";
import { strings } from "../strings";
import { Icon } from "./Icon";
import { ErrorNote } from "./ErrorNote";

/** The side panel with all chats of one character. */
export function ChatList({
  characterId,
  currentChatId,
  refreshKey,
  disabled,
  onOpen,
  onNew,
  onDeleted,
  onClose,
}: {
  characterId: number;
  currentChatId: number;
  /** Changes when the list must load again. */
  refreshKey: string;
  disabled: boolean;
  onOpen: (chatId: number) => void;
  onNew: () => void;
  onDeleted: (chatId: number) => void;
  onClose: () => void;
}) {
  const [chats, setChats] = useState<ChatSummary[]>([]);
  const [confirm, setConfirm] = useState<number | null>(null);
  const [error, setError] = useState<UiError | null>(null);

  useEffect(() => {
    let active = true;
    listChats(characterId)
      .then((list) => {
        if (active) setChats(list);
      })
      .catch((e: unknown) => {
        if (active) setError(toUiError(e));
      });
    return () => {
      active = false;
    };
  }, [characterId, refreshKey]);

  const remove = (chatId: number) => {
    deleteChat(chatId)
      .then(() => {
        setConfirm(null);
        setChats((list) => list.filter((c) => c.id !== chatId));
        onDeleted(chatId);
      })
      .catch((e: unknown) => {
        setError(toUiError(e));
      });
  };

  return (
    <aside className="chat-list" aria-label={strings.chats.title}>
      <div className="chat-list-head">
        <h2>{strings.chats.title}</h2>
        <button type="button" className="link-button" onClick={onClose}>
          <Icon name="close" />
          {strings.chats.close}
        </button>
      </div>
      <button type="button" className="button" onClick={onNew} disabled={disabled}>
        {strings.chats.newChat}
      </button>
      {error && <ErrorNote error={error} />}
      <ul>
        {chats.map((c) => (
          <li key={c.id} className={c.id === currentChatId ? "chat-item chat-item-current" : "chat-item"}>
            <button
              type="button"
              className="chat-item-open"
              disabled={disabled}
              onClick={() => {
                onOpen(c.id);
              }}
            >
              <span className="chat-item-title">{c.title || c.preview || strings.chats.untitled}</span>
              <span className="chat-item-meta">{strings.chats.messages(c.message_count)}</span>
            </button>
            {confirm === c.id ? (
              <span className="confirm">
                <span>{strings.chats.confirmDelete}</span>
                <button
                  type="button"
                  className="link-button danger"
                  onClick={() => {
                    remove(c.id);
                  }}
                >
                  {strings.chats.yes}
                </button>
                <button
                  type="button"
                  className="link-button"
                  onClick={() => {
                    setConfirm(null);
                  }}
                >
                  {strings.chats.no}
                </button>
              </span>
            ) : (
              <button
                type="button"
                className="link-button"
                disabled={disabled}
                aria-label={strings.chats.delete}
                onClick={() => {
                  setConfirm(c.id);
                }}
              >
                {strings.message.delete}
              </button>
            )}
          </li>
        ))}
      </ul>
    </aside>
  );
}
