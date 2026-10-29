import { useState } from "react";
import type { ChatView, Persona, StyleSettings } from "../api";
import { strings } from "../strings";
import { Avatar } from "./Avatar";
import { StylePanel } from "./StylePanel";

const TONES = ["casual", "wholesome", "dramatic", "flirty", "dark", "funny", "custom"];
const MAX_CUSTOM_TONE = 300;

export function ChatHeader({
  view,
  personas,
  locked,
  onBack,
  onPersona,
  onTone,
  onStyle,
  onToggleChats,
}: {
  view: ChatView;
  personas: Persona[];
  locked: boolean;
  onBack: () => void;
  onPersona: (personaId: number | null) => void;
  onTone: (tone: string, custom: string) => void;
  onStyle: (style: string, custom: StyleSettings | null) => void;
  onToggleChats: () => void;
}) {
  const { chat, character } = view;
  const [toneDraft, setToneDraft] = useState<string | null>(null);
  const [styleOpen, setStyleOpen] = useState(false);

  return (
    <header className="chat-header">
      <button type="button" className="icon-button back" aria-label={strings.chat.back} onClick={onBack}>
        ‹
      </button>
      <Avatar kind="character" owner={character} />
      <div className="chat-title">
        <h1>{character.name}</h1>
        <p>{character.tagline}</p>
      </div>
      <div className="chat-controls">
        <label className="control">
          <span>{strings.header.persona}</span>
          <select
            value={chat.persona_id ?? ""}
            disabled={locked}
            onChange={(e) => {
              onPersona(e.target.value === "" ? null : Number(e.target.value));
            }}
          >
            <option value="">{strings.header.noPersona}</option>
            {personas.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
        </label>
        <label className="control">
          <span>{strings.header.tone}</span>
          <select
            value={chat.tone}
            disabled={locked}
            onChange={(e) => {
              const tone = e.target.value;
              if (tone === "custom") {
                setToneDraft(chat.tone_custom);
              } else {
                setToneDraft(null);
                onTone(tone, "");
              }
            }}
          >
            {TONES.map((t) => (
              <option key={t} value={t}>
                {strings.tones.get(t) ?? t}
              </option>
            ))}
          </select>
        </label>
        <div className="control-anchor">
          <button
            type="button"
            className="button button-quiet"
            disabled={locked}
            aria-expanded={styleOpen}
            onClick={() => {
              setStyleOpen((v) => !v);
            }}
          >
            {strings.header.style}: {strings.styles.get(chat.reply_style) ?? chat.reply_style}
          </button>
          {styleOpen && (
            <StylePanel
              style={chat.reply_style}
              customJson={chat.reply_custom}
              onClose={() => {
                setStyleOpen(false);
              }}
              onApply={(style, custom) => {
                setStyleOpen(false);
                onStyle(style, custom);
              }}
            />
          )}
        </div>
        <button type="button" className="button button-quiet" onClick={onToggleChats}>
          {strings.header.chats}
        </button>
      </div>
      {toneDraft !== null && (
        <form
          className="tone-form"
          onSubmit={(e) => {
            e.preventDefault();
            if (toneDraft.trim()) {
              onTone("custom", toneDraft);
              setToneDraft(null);
            }
          }}
        >
          <label>
            <span>{strings.header.customTone}</span>
            <input
              type="text"
              value={toneDraft}
              maxLength={MAX_CUSTOM_TONE}
              placeholder={strings.header.customTonePlaceholder}
              onChange={(e) => {
                setToneDraft(e.target.value);
              }}
            />
          </label>
          <button
            type="button"
            className="button button-quiet"
            onClick={() => {
              setToneDraft(null);
            }}
          >
            {strings.message.cancel}
          </button>
          <button type="submit" className="button" disabled={!toneDraft.trim()}>
            {strings.header.apply}
          </button>
        </form>
      )}
    </header>
  );
}
