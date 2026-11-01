import { useEffect, useMemo, useState, type ReactNode } from "react";
import {
  deleteCharacter,
  getCharacter,
  modelsPage,
  saveCharacter,
  setCharacterModel,
  suggestAnchor,
  toUiError,
  type AvatarChange,
  type Character,
  type CharacterFields,
  type ImportDraft,
  type ImportNote,
  type Staged,
  type UiError,
} from "../api";
import { CARD_BUDGET, checkCard } from "../cardCheck";
import { strings } from "../strings";
import { Avatar } from "./Avatar";
import { CropAvatar } from "./CropAvatar";
import { PictureField } from "./PictureField";
import { startingCustom, StyleEditor } from "./StylePanel";
import { TestChat } from "./TestChat";
import { ErrorNote } from "./ErrorNote";

const TONES = ["casual", "wholesome", "dramatic", "flirty", "dark", "funny"];

const EMPTY: CharacterFields = {
  name: "",
  tagline: "",
  personality: "",
  scenario: "",
  greeting: "",
  example_dialogue: "",
  voice_anchor: "",
  appearance_tags: "",
  art_style: "",
  default_tone: "casual",
  default_reply_style: "conversational",
  default_reply_custom: "",
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
    art_style: c.art_style,
    default_tone: c.default_tone,
    default_reply_style: c.default_reply_style,
    default_reply_custom: c.default_reply_custom,
    age: c.age,
    is_adult: c.is_adult,
  };
}

function noteText(note: ImportNote): string {
  const t = strings.importNotes;
  switch (note.kind) {
    case "cut":
      return t.cut(t.fieldNames.get(note.field) ?? note.field);
    case "adult":
      return t.adult;
    case "no_age":
      return t.noAge;
    case "dropped_prompts":
      return t.droppedPrompts;
    case "more_greetings":
      return t.moreGreetings(note.count);
    case "world":
      return t.world(note.count, note.dropped);
    case "no_picture":
      return t.noPicture;
  }
}

/** A centered square that fills the shorter side. */
function centerCrop(s: Staged) {
  const size = Math.min(s.width, s.height);
  return { x: Math.round((s.width - size) / 2), y: Math.round((s.height - size) / 2), size };
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
  draft,
  picture,
  ready,
  onSaved,
  onDeleted,
  onCancel,
}: {
  characterId: number | null;
  /** An imported card that is not saved yet. */
  draft: ImportDraft | null;
  /** A staged new profile picture, for example from the gallery. */
  picture?: Staged | null;
  ready: boolean;
  onSaved: (character: Character) => void;
  onDeleted: () => void;
  onCancel: () => void;
}) {
  const [saved, setSaved] = useState<Character | null>(null);
  const [fields, setFields] = useState<CharacterFields>(draft?.fields ?? EMPTY);
  const first = draft?.picture ?? picture ?? null;
  const [avatar, setAvatar] = useState<AvatarChange>(
    first ? { kind: "staged", token: first.token, crop: centerCrop(first) } : { kind: "keep" },
  );
  const [error, setError] = useState<UiError | null>(null);
  const [saving, setSaving] = useState(false);
  const [suggesting, setSuggesting] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [testing, setTesting] = useState(false);
  const [staged, setStaged] = useState<Staged | null>(first);
  const [model, setModel] = useState<string | null>(null);
  const [models, setModels] = useState<string[]>([]);
  const [defaultModel, setDefaultModel] = useState("");

  useEffect(() => {
    let active = true;
    modelsPage()
      .then((p) => {
        if (!active) return;
        setModels(p.models.filter((m) => m.installed).map((m) => m.id));
        setDefaultModel(p.default_model);
      })
      .catch(() => {
        // Without the list, the creator has no model picker.
      });
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    if (characterId === null) return;
    getCharacter(characterId)
      .then((c) => {
        setSaved(c);
        setFields(fieldsOf(c));
        setModel(c.model_override);
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
    saveCharacter(saved?.id ?? null, fields, avatar, saved ? null : (draft?.extras ?? null))
      .then(async (c) => {
        if (c.model_override !== model) {
          await setCharacterModel(c.id, model);
          onSaved({ ...c, model_override: model });
        } else {
          onSaved(c);
        }
      })
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

        {draft && draft.notes.length > 0 && (
          <section className="notes" aria-label={strings.importNotes.title}>
            <strong>{strings.importNotes.title}</strong>
            <ul>
              {draft.notes.map((note, i) => (
                <li key={i}>{noteText(note)}</li>
              ))}
            </ul>
          </section>
        )}

        <div className="creator-top">
          <div className="field">
            <span className="field-label">{strings.creator.picture}</span>
            <PictureField
              kind="character"
              owner={saved}
              value={avatar}
              onChange={setAvatar}
              initial={first}
              onStaged={setStaged}
            />
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
                value={fields.age === 0 ? "" : fields.age}
                onChange={(e) => {
                  set("age", e.target.value === "" ? 0 : Math.trunc(Number(e.target.value)));
                }}
              />
            </Field>
          </div>
          <div className="field card-preview" aria-label={strings.creator.preview}>
            <span className="field-label">{strings.creator.preview}</span>
            <div className="character-card">
              {avatar.kind === "staged" && staged ? (
                <CropAvatar staged={staged} crop={avatar.crop} label={fields.name} />
              ) : (
                <Avatar
                  kind="character"
                  owner={avatar.kind === "remove" ? null : saved}
                  label={fields.name}
                  size="large"
                />
              )}
              <span className="card-name">{fields.name.trim() || strings.creator.previewName}</span>
              <span className="card-tagline">{fields.tagline}</span>
            </div>
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

        <details className="more" open={saved !== null || draft !== null}>
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
          <Field label={strings.creator.artStyle} hint={strings.creator.artStyleHint}>
            <input
              type="text"
              maxLength={120}
              value={fields.art_style}
              onChange={(e) => {
                set("art_style", e.target.value);
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
          </div>
          <div className="field creator-style">
            <StyleEditor
              picked={fields.default_reply_style}
              custom={startingCustom(fields.default_reply_style, fields.default_reply_custom)}
              name="default-reply-style"
              legend={strings.creator.style}
              onChange={(picked, custom) => {
                setFields((f) => ({
                  ...f,
                  default_reply_style: picked,
                  default_reply_custom: picked === "custom" ? JSON.stringify(custom) : "",
                }));
              }}
            />
            <span className="field-hint">{strings.creator.styleHint}</span>
          </div>
          {models.length > 1 && (
            <Field label={strings.creator.model} hint={strings.creator.modelHint}>
              <select
                value={model ?? ""}
                onChange={(e) => {
                  setModel(e.target.value === "" ? null : e.target.value);
                }}
              >
                <option value="">{strings.header.defaultModel(defaultModel)}</option>
                {models.map((m) => (
                  <option key={m} value={m}>
                    {m}
                  </option>
                ))}
              </select>
            </Field>
          )}
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

        {error && <ErrorNote error={error} />}

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
