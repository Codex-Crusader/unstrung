-- Phase 5: generated pictures.
-- file_path is relative to the gallery folder in the library folder, which
-- the user picks, so the database stays valid when that folder moves.
CREATE TABLE images (
    id              INTEGER PRIMARY KEY,
    chat_id         INTEGER NOT NULL REFERENCES chats (id) ON DELETE CASCADE,
    message_id      INTEGER REFERENCES messages (id) ON DELETE SET NULL,
    file_path       TEXT    NOT NULL,
    prompt          TEXT    NOT NULL,
    negative_prompt TEXT    NOT NULL DEFAULT '',
    seed            INTEGER NOT NULL,
    image_model     TEXT    NOT NULL,
    ref_mode        TEXT    NOT NULL DEFAULT 'tags',
    width           INTEGER NOT NULL,
    height          INTEGER NOT NULL,
    -- 1 when the picture was made as an adult picture. The gallery can blur these.
    adult           INTEGER NOT NULL DEFAULT 0,
    created_at      INTEGER NOT NULL
);

CREATE INDEX images_by_chat ON images (chat_id, created_at);
CREATE INDEX images_by_message ON images (message_id);

-- The art style of a character's pictures, for example "anime" or "oil painting".
ALTER TABLE characters ADD COLUMN art_style TEXT NOT NULL DEFAULT '';
