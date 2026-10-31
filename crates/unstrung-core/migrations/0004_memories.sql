-- What a chat remembers (Phase 7): the user's goal, the story so far and facts.
-- Each memory belongs to the message it came from. The prompt uses only
-- memories whose source message is on the chat's current path, so a rewind,
-- a regenerate or a branch hides them without a rebuild. A summary covers
-- the path up to its source message, and the newest one on the path wins.
-- The same holds for a goal.
CREATE TABLE memories (
    id                 INTEGER PRIMARY KEY,
    chat_id            INTEGER NOT NULL REFERENCES chats (id) ON DELETE CASCADE,
    kind               TEXT    NOT NULL CHECK (kind IN ('goal', 'summary', 'fact')),
    content            TEXT    NOT NULL,
    source_message_id  INTEGER NOT NULL REFERENCES messages (id) ON DELETE CASCADE,
    -- 'auto' from the memory pass, 'user' from "Remember this" or the panel.
    origin             TEXT    NOT NULL DEFAULT 'auto' CHECK (origin IN ('auto', 'user')),
    is_pinned          INTEGER NOT NULL DEFAULT 0,
    -- The user removed it in the panel. Kept, so the pass does not add it again.
    is_hidden          INTEGER NOT NULL DEFAULT 0,
    created_at         INTEGER NOT NULL
);

CREATE INDEX memories_by_chat ON memories (chat_id, kind);
