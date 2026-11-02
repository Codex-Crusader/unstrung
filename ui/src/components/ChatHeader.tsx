import { useRef, useState, type ReactNode } from "react";
import type { ChatView, Persona, StyleSettings } from "../api";
import { strings } from "../strings";
import { Avatar } from "./Avatar";
import { Icon, type IconName } from "./Icon";
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
  onModel,
  models,
  defaultModel,
  onToggleChats,
  onToggleMemory,
  chatsOpen,
  memoryOpen,
  refresh,
}: {
  view: ChatView;
  personas: Persona[];
  locked: boolean;
  onBack: () => void;
  onPersona: (personaId: number | null) => void;
  onTone: (tone: string, custom: string) => void;
  onStyle: (style: string, custom: StyleSettings | null) => void;
  onModel: (model: string | null) => void;
  /** The installed models. */
  models: { id: string; name: string }[];
  defaultModel: string;
  onToggleChats: () => void;
  onToggleMemory: () => void;
  chatsOpen: boolean;
  memoryOpen: boolean;
  /** The Refresh menu. */
  refresh: {
    context: () => void;
    editSummary: () => void;
    nextChapter: () => void;
    restart: () => void;
    reload: () => void;
  };
}) {
  const { chat, character } = view;
  const [toneDraft, setToneDraft] = useState<string | null>(null);
  const [styleOpen, setStyleOpen] = useState(false);
  const modelName = (id: string) => models.find((m) => m.id === id)?.name ?? id;
  const menu = useRef<HTMLDetailsElement>(null);
  const item = (icon: IconName, label: string, hint: string, run: () => void): ReactNode => (
    <li key={label}>
      <button
        type="button"
        className="menu-item"
        title={hint}
        onClick={() => {
          menu.current?.removeAttribute("open");
          run();
        }}
      >
        <Icon name={icon} />
        {label}
      </button>
    </li>
  );
  const r = strings.refresh;

  return (
    <header className="chat-header">
      <div className="chat-header-top">
        <button type="button" className="icon-button back" aria-label={strings.chat.back} onClick={onBack}>
          <Icon name="back" />
        </button>
        <Avatar kind="character" owner={character} />
        <div className="chat-title">
          <h1>{character.name}</h1>
          <p>{character.tagline}</p>
        </div>
        {/* The side panels. A pressed button shows that its panel is open. */}
        <div className="chat-panels">
          {!locked && (
            <details className="menu refresh-menu" ref={menu}>
              <summary className="button button-quiet panel-toggle">
                <Icon name="refresh" />
                {r.button}
              </summary>
              <ul className="menu-list">
                {item("refresh", r.context, r.contextHint, refresh.context)}
                {item("edit", r.editSummary, r.editSummaryHint, refresh.editSummary)}
                {item("book", r.nextChapter, r.nextChapterHint, refresh.nextChapter)}
                {item("bubble", r.restart, r.restartHint, refresh.restart)}
                {item("models", r.reload, r.reloadHint, refresh.reload)}
              </ul>
            </details>
          )}
          <button
            type="button"
            className="button button-quiet panel-toggle"
            aria-pressed={memoryOpen}
            onClick={onToggleMemory}
          >
            <Icon name="memory" />
            {strings.memory.button}
          </button>
          <button
            type="button"
            className="button button-quiet panel-toggle"
            aria-pressed={chatsOpen}
            onClick={onToggleChats}
          >
            <Icon name="chats" />
            {strings.header.chats}
          </button>
        </div>
      </div>
      <div className="chat-settings">
        <label className="control">
          <span>
            <Icon name="personas" />
            {strings.header.persona}
          </span>
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
          <span>
            <Icon name="tone" />
            {strings.header.tone}
          </span>
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
        <div className="control control-anchor">
          <span id="style-label">
            <Icon name="style" />
            {strings.header.style}
          </span>
          <button
            type="button"
            className="select-button"
            disabled={locked}
            aria-labelledby="style-label style-value"
            aria-expanded={styleOpen}
            onClick={() => {
              setStyleOpen((v) => !v);
            }}
          >
            <span id="style-value">{strings.styles.get(chat.reply_style) ?? chat.reply_style}</span>
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
        {models.length > 1 && (
          <label className="control">
            <span>
              <Icon name="models" />
              {strings.header.model}
            </span>
            <select
              value={chat.chat_model ?? ""}
              disabled={locked}
              onChange={(e) => {
                onModel(e.target.value === "" ? null : e.target.value);
              }}
            >
              <option value="">{strings.header.defaultModel(modelName(character.model_override ?? defaultModel))}</option>
              {models.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.name}
                </option>
              ))}
            </select>
          </label>
        )}
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
