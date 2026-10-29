-- Phase 2: characters, personas, reply styles, branches.

-- Custom tone text and custom reply style sliders (JSON), per chat.
ALTER TABLE chats ADD COLUMN tone_custom TEXT NOT NULL DEFAULT '';
ALTER TABLE chats ADD COLUMN reply_custom TEXT NOT NULL DEFAULT '';

-- Deleted messages stay until the undo window ends. A later cleanup removes them.
ALTER TABLE messages ADD COLUMN deleted_at INTEGER;

-- The extensions block of an imported character card, kept for export.
ALTER TABLE characters ADD COLUMN extensions_json TEXT NOT NULL DEFAULT '{}';

-- World info is used in a later version. Imported cards keep their entries now.
CREATE TABLE world_info (
    id           INTEGER PRIMARY KEY,
    character_id INTEGER NOT NULL REFERENCES characters (id) ON DELETE CASCADE,
    keywords     TEXT    NOT NULL,
    content      TEXT    NOT NULL,
    is_enabled   INTEGER NOT NULL DEFAULT 1
);

CREATE INDEX world_info_by_character ON world_info (character_id);
