import { useEffect, useMemo, useState, type ReactNode } from "react";
import {
  deleteCharacter,
  getCharacter,
  saveCharacter,
  suggestAnchor,
  toUiError,
  type AvatarChange,
  type Character,
  type CharacterFields,
  type UiError,
} from "../api";
import { CARD_BUDGET, checkCard } from "../cardCheck";
import { strings } from "../strings";
import { PictureField } from "./PictureField";
import { TestChat } from "./TestChat";

const TONES = ["casual", "wholesome", "dramatic", "flirty", "dark", "funny"];
const STYLES = ["texting", "conversational", "storyteller", "novel"];

const EMPTY: CharacterFields = {
  name: "",
  tagline: "",
  personality: "",
  scenario: "",
  greeting: "",
  example_dialogue: "",
  voice_anchor: "",
  appearance_tags: "",
  default_tone: "casual",
  default_reply_style: "conversational",
  age: 25,
  is_adult: false,
};

function fieldsOf(c: Character): CharacterFields {
  return {
    name: c.name,
    tagline: c.tagline,
    personality: c.personality,
    scenario: c.scenario,
    greeting: c.greeting,
    example_dialogue: c.example_dialogue,
    voice_anchor: c.voice_anchor,
    appearance_tags: c.appearance_tags,
    default_tone: c.default_tone,
    default_reply_style: c.default_reply_style,
    age: c.age,
    is_adult: c.is_adult,
  };
}

function Field({
  label,
  hint,
  required,
  children,
}: {
  label: string;
  hint?: string;
  required?: boolean;
  children: ReactNode;
}) {
  return (
    <label className="field">
      <span className="field-label">
        {label}
        {required && <span className="field-required"> · {strings.creator.required}</span>}
      </span>
      {children}
      {hint && <span className="field-hint">{hint}</span>}
    </label>
  );
}

export function CreatorScreen({
  characterId,
  ready,
  onSaved,
  onDeleted,
  onCancel,
}: {
  characterId: number | null;
  ready: boolean;
  onSaved: (character: Character) => void;
  onDeleted: () => void;
  onCancel: () => void;
}) {
  const [saved, setSaved] = useState<Character | null>(null);
  const [fields, setFields] = useState<CharacterFields>(EMPTY);
  const [avatar, setAvatar] = useState<AvatarChange>({ kind: "keep" });
  const [error, setError] = useState<UiError | null>(null);
  const [saving, setSaving] = useState(false);
  const [suggesting, setSuggesting] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [testing, setTesting] = useState(false);

  useEffect(() => {
    if (characterId === null) return;
    getCharacter(characterId)
      .then((c) => {
        setSaved(c);
        setFields(fieldsOf(c));
      })
      .catch((e: unknown) => {
        setError(toUiError(e));
      });
  }, [characterId]);

  const check = useMemo(() => checkCard(fields), [fields]);
  const set = <K extends keyof CharacterFields>(key: K, value: CharacterFields[K]) => {
    setFields((f) => ({ ...f, [key]: value }));
  };
  const complete =
    fields.name.trim() !== "" && fields.personality.trim() !== "" && fields.greeting.trim() !== "" && fields.age >= 1;

  const save = () => {
    if (!complete || saving) return;
    setSaving(true);
    setError(null);
    saveCharacter(saved?.id ?? null, fields, avatar)
      .then(onSaved)
      .catch((e: unknown) => {
        setError(toUiError(e));
      })
      .finally(() => {
        setSaving(false);
      });
  };

  const suggest = () => {
    setSuggesting(true);
    setError(null);
    suggestAnchor(fields.name, fields.personality)
      .then((anchor) => {
        set("voice_anchor", anchor);
      })
      .catch((e: unknown) => {
        setError(toUiError(e));
      })
      .finally(() => {
        setSuggesting(false);
      });
  };

  const remove = () => {
    if (!saved) return;
    deleteCharacter(saved.id)
      .then(onDeleted)
      .catch((e: unknown) => {
        setError(toUiError(e));
      });
  };

  return (
    <div className="creator-layout">
      <form
        className="page creator"
        onSubmit={(e) => {
          e.preventDefault();
          save();
        }}
      >
        <h1>{saved ? strings.creator.editTitle(saved.name) : strings.creator.newTitle}</h1>

        <div className="creator-top">
          <div className="field">
            <span className="field-label">{strings.creator.picture}</span>
            <PictureField kind="character" owner={saved} value={avatar} onChange={setAvatar} />
          </div>
          <div className="creator-main">
            <Field label={strings.creator.name} required>
              <input
                type="text"
                value={fields.name}
                maxLength={80}
                placeholder={strings.creator.namePlaceholder}
                onChange={(e) => {
                  set("name", e.target.value);
                }}
              />
            </Field>
            <Field label={strings.creator.age} required>
              <input
                type="number"
                min={1}
                max={10000}
                value={fields.age}
                onChange={(e) => {
                  set("age", Math.trunc(Number(e.target.value)));
                }}
              />
            </Field>
          </div>
        </div>

        <Field label={strings.creator.personality} hint={strings.creator.personalityHint} required>
          <textarea
            rows={4}
            maxLength={4000}
            value={fields.personality}
            onChange={(e) => {
              set("personality", e.target.value);
            }}
          />
        </Field>
        <Field label={strings.creator.greeting} hint={strings.creator.greetingHint} required>
          <textarea
            rows={3}
            maxLength={3000}
            value={fields.greeting}
            onChange={(e) => {
              set("greeting", e.target.value);
            }}
          />
        </Field>

        <details className="more" open={saved !== null}>
          <summary>{strings.creator.more}</summary>
          <Field label={strings.creator.tagline} hint={strings.creator.taglineHint}>
            <input
              type="text"
              maxLength={160}
              value={fields.tagline}
              onChange={(e) => {
                set("tagline", e.target.value);
              }}
            />
          </Field>
          <Field label={strings.creator.scenario} hint={strings.creator.scenarioHint}>
            <textarea
              rows={3}
              maxLength={3000}
              value={fields.scenario}
              onChange={(e) => {
                set("scenario", e.target.value);
              }}
            />
          </Field>
          <Field label={strings.creator.examples} hint={strings.creator.examplesHint}>
            <textarea
              rows={5}
              maxLength={6000}
              value={fields.example_dialogue}
              onChange={(e) => {
                set("example_dialogue", e.target.value);
              }}
            />
          </Field>
          <Field label={strings.creator.voice} hint={strings.creator.voiceHint}>
            <textarea
              rows={2}
              maxLength={400}
              value={fields.voice_anchor}
              onChange={(e) => {
                set("voice_anchor", e.target.value);
              }}
            />
          </Field>
          <button
            type="button"
            className="link-button"
            disabled={!ready || suggesting || fields.personality.trim() === ""}
            onClick={suggest}
          >
            {suggesting ? strings.creator.suggesting : strings.creator.suggest}
          </button>
          <Field label={strings.creator.tags} hint={strings.creator.tagsHint}>
            <input
              type="text"
              maxLength={400}
              value={fields.appearance_tags}
              onChange={(e) => {
                set("appearance_tags", e.target.value);
              }}
            />
          </Field>
          <div className="field-row">
            <Field label={strings.creator.tone}>
              <select
                value={fields.default_tone}
                onChange={(e) => {
                  set("default_tone", e.target.value);
                }}
              >
                {TONES.map((t) => (
                  <option key={t} value={t}>
                    {strings.tones.get(t) ?? t}
                  </option>
                ))}
              </select>
            </Field>
            <Field label={strings.creator.style}>
              <select
                value={fields.default_reply_style}
                onChange={(e) => {
                  set("default_reply_style", e.target.value);
                }}
              >
                {STYLES.map((s) => (
                  <option key={s} value={s}>
                    {strings.styles.get(s) ?? s}
                  </option>
                ))}
              </select>
            </Field>
          </div>
        </details>

        <section className="tips" aria-label={strings.creator.tips}>
          <p className="field-hint">{strings.creator.tokens(check.tokens, CARD_BUDGET)}</p>
          {check.tips.length > 0 && (
            <ul>
              {check.tips.map((tip) => (
                <li key={tip.key}>{tip.text}</li>
              ))}
            </ul>
          )}
        </section>

        {error && (
          <p className="chat-error" role="alert">
            {error.message}
          </p>
        )}

        <div className="form-actions">
          {saved &&
            (confirmDelete ? (
              <span className="confirm">
                <span>{strings.creator.confirmDelete(saved.name)}</span>
                <button type="button" className="link-button danger" onClick={remove}>
                  {strings.creator.yes}
                </button>
                <button
                  type="button"
                  className="link-button"
                  onClick={() => {
                    setConfirmDelete(false);
                  }}
                >
                  {strings.creator.no}
                </button>
              </span>
            ) : (
              <button
                type="button"
                className="link-button danger"
                onClick={() => {
                  setConfirmDelete(true);
                }}
              >
                {strings.creator.delete}
              </button>
            ))}
          <span className="spacer" />
          <button
            type="button"
            className="button button-quiet"
            disabled={!complete || !ready}
            title={complete ? undefined : strings.creator.testFirst}
            onClick={() => {
              setTesting(true);
            }}
          >
            {strings.creator.test}
          </button>
          <button type="button" className="button button-quiet" onClick={onCancel}>
            {strings.creator.cancel}
          </button>
          <button type="submit" className="button" disabled={!complete || saving}>
            {saving ? strings.creator.saving : strings.creator.save}
          </button>
        </div>
      </form>
      {testing && (
        <TestChat
          fields={fields}
          ready={ready}
          onClose={() => {
            setTesting(false);
          }}
        />
      )}
    </div>
  );
}
