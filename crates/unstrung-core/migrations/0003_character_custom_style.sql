-- A character's own custom reply style, as StyleSettings JSON. New chats copy
-- it when the default reply style is "custom". Empty for the presets.
ALTER TABLE characters ADD COLUMN default_reply_custom TEXT NOT NULL DEFAULT '';
