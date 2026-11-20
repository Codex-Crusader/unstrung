-- Character growth. The genome is the card's traits as numbers, made once
-- per card version (card_hash) and shared by every chat with the character.
-- A growth state is how the character feels about the user after one reply.
-- It belongs to that reply, so a rewind, regenerate or branch picks the
-- right state from the path with no rebuild, and the primary key on the
-- message makes a second appraisal of the same reply impossible.
CREATE TABLE character_genomes (
    character_id  INTEGER PRIMARY KEY REFERENCES characters (id) ON DELETE CASCADE,
    card_hash     TEXT    NOT NULL,
    -- JSON of growth::Genome.
    genome        TEXT    NOT NULL,
    created_at    INTEGER NOT NULL
);

CREATE TABLE growth_states (
    message_id  INTEGER PRIMARY KEY REFERENCES messages (id) ON DELETE CASCADE,
    chat_id     INTEGER NOT NULL REFERENCES chats (id) ON DELETE CASCADE,
    -- JSON of growth::State after this reply, and of the checked
    -- growth::Appraisal that led to it.
    state       TEXT    NOT NULL,
    appraisal   TEXT    NOT NULL,
    created_at  INTEGER NOT NULL
);

CREATE INDEX growth_states_by_chat ON growth_states (chat_id);
