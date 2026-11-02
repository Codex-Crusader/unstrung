-- Facts that a character keeps for new chats (Phase 7). The user keeps them
-- from a chat's memory panel, and they outlive that chat. A chat uses them
-- when chats.use_character_memory is 1.
CREATE TABLE character_memories (
    id            INTEGER PRIMARY KEY,
    character_id  INTEGER NOT NULL REFERENCES characters (id) ON DELETE CASCADE,
    content       TEXT    NOT NULL,
    created_at    INTEGER NOT NULL,
    UNIQUE (character_id, content)
);

-- Whether a new chat with the character uses them: never, after a
-- question, or always.
ALTER TABLE characters ADD COLUMN memory_mode TEXT NOT NULL DEFAULT 'ask'
    CHECK (memory_mode IN ('off', 'ask', 'always'));
