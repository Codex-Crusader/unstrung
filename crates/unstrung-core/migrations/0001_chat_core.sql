-- Phase 1 schema. Later phases add tables and columns. They never rename these.
-- Times are Unix milliseconds. File paths are relative to the app folder.

CREATE TABLE characters (
    id                  INTEGER PRIMARY KEY,
    name                TEXT    NOT NULL,
    avatar_path         TEXT,
    tagline             TEXT    NOT NULL DEFAULT '',
    personality         TEXT    NOT NULL DEFAULT '',
    scenario            TEXT    NOT NULL DEFAULT '',
    greeting            TEXT    NOT NULL DEFAULT '',
    example_dialogue    TEXT    NOT NULL DEFAULT '',
    voice_anchor        TEXT    NOT NULL DEFAULT '',
    appearance_tags     TEXT    NOT NULL DEFAULT '',
    default_tone        TEXT    NOT NULL DEFAULT 'casual',
    default_reply_style TEXT    NOT NULL DEFAULT 'conversational',
    model_override      TEXT,
    age                 INTEGER NOT NULL,
    is_adult            INTEGER NOT NULL DEFAULT 0,
    starter_key         TEXT    UNIQUE,
    created_at          INTEGER NOT NULL
);

CREATE TABLE personas (
    id              INTEGER PRIMARY KEY,
    name            TEXT    NOT NULL,
    avatar_path     TEXT,
    description     TEXT    NOT NULL DEFAULT '',
    appearance_tags TEXT    NOT NULL DEFAULT '',
    age             INTEGER,
    is_default      INTEGER NOT NULL DEFAULT 0,
    starter_key     TEXT    UNIQUE,
    created_at      INTEGER NOT NULL
);

CREATE TABLE chats (
    id                     INTEGER PRIMARY KEY,
    character_id           INTEGER NOT NULL REFERENCES characters (id) ON DELETE CASCADE,
    persona_id             INTEGER REFERENCES personas (id) ON DELETE SET NULL,
    title                  TEXT    NOT NULL DEFAULT '',
    tone                   TEXT    NOT NULL,
    reply_style            TEXT    NOT NULL,
    chat_model             TEXT,
    head_message_id        INTEGER,
    context_start_id       INTEGER,
    continued_from_chat_id INTEGER REFERENCES chats (id) ON DELETE SET NULL,
    summary                TEXT    NOT NULL DEFAULT '',
    summary_upto_id        INTEGER,
    use_character_memory   INTEGER NOT NULL DEFAULT 0,
    deleted_at             INTEGER,
    created_at             INTEGER NOT NULL,
    updated_at             INTEGER NOT NULL
);

CREATE INDEX chats_by_character ON chats (character_id, updated_at);

-- Messages form a tree. Rewind, regenerate, edit and branch all move chats.head_message_id.
CREATE TABLE messages (
    id                        INTEGER PRIMARY KEY,
    chat_id                   INTEGER NOT NULL REFERENCES chats (id) ON DELETE CASCADE,
    parent_id                 INTEGER REFERENCES messages (id) ON DELETE CASCADE,
    role                      TEXT    NOT NULL CHECK (role IN ('user', 'character', 'system', 'ooc')),
    persona_id                INTEGER REFERENCES personas (id) ON DELETE SET NULL,
    content                   TEXT    NOT NULL,
    raw_content               TEXT,
    flagged_out_of_character  INTEGER NOT NULL DEFAULT 0,
    image_id                  INTEGER,
    token_count               INTEGER,
    model                     TEXT,
    prompt_version            INTEGER,
    sampler_json              TEXT,
    created_at                INTEGER NOT NULL
);

CREATE INDEX messages_by_parent ON messages (parent_id);
CREATE INDEX messages_by_chat ON messages (chat_id);

CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
