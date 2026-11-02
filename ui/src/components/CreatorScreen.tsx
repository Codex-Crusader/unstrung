import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  deleteCharacter,
  getCharacter,
  modelsPage,
  saveCharacter,
  setCharacterModel,
  suggestAnchor,
  suggestLooks,
  suggestLooksFromPicture,
  canReadPictures,
  toUiError,
  type AvatarChange,
  type Character,
  type CharacterFields,
  type ImportDraft,
  type ImportNote,
  type Staged,
  type UiError,
  stopReply,
  getAdultContent,
} from "../api";
import { CARD_BUDGET, checkCard, needsPersonWord } from "../cardCheck";
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
  outfit: "",
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
    outfit: c.outfit,
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
  warn,
  required,
  children,
}: {
  label: string;
  hint?: string;
  /** Shows the hint as a warning. */
  warn?: boolean;
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
      {hint && <span className={warn ? "field-hint warn" : "field-hint"}>{hint}</span>}
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
  // The 18+ checkbox shows only with adult content on, or on a card that has it.
  const [adultOn, setAdultOn] = useState(false);
  useEffect(() => {
    getAdultContent()
      .then(setAdultOn)
      .catch(() => {
        setAdultOn(false);
      });
  }, []);
  const [saving, setSaving] = useState(false);
  const [suggesting, setSuggesting] = useState(false);
  const [lookingUp, setLookingUp] = useState(false);
  // A Looks suggestion holds the AI slot. Closing the creator stops it, so a
  // chat message right after does not fail as busy.
  const lookingUpNow = useRef(false);
  useEffect(() => {
    lookingUpNow.current = lookingUp;
  }, [lookingUp]);
  useEffect(
    () => () => {
      if (lookingUpNow.current) void stopReply().catch(() => undefined);
    },
    [],
  );
  const [lookedUp, setLookedUp] = useState(false);
  const [canSee, setCanSee] = useState(false);
  const autoLooks = useRef(false);
  /** The fields as the screen shows them now, for a suggestion that comes back later. */
  const shown = useRef(fields);
  useEffect(() => {
    shown.current = fields;
  });
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

  /** The card's own text, where most cards describe looks. */
  const cardText = [fields.personality, fields.scenario, fields.greeting, fields.example_dialogue]
    .filter((t) => t.trim() !== "")
    .join("\n\n");

  /** Puts suggested looks in the form. `onlyEmpty`: keep what the user or the card wrote. */
  const fill = (onlyEmpty: boolean) => (s: { looks: string; outfit: string }) => {
    const keep = (old: string, next: string) => (next === "" || (onlyEmpty && old.trim() !== "") ? old : next);
    const now = shown.current;
    // The note says the fields were filled, so only when one really changes.
    if (keep(now.appearance_tags, s.looks) !== now.appearance_tags || keep(now.outfit, s.outfit) !== now.outfit) {
      setLookedUp(true);
    }
    setFields((f) => ({
      ...f,
      appearance_tags: keep(f.appearance_tags, s.looks),
      outfit: keep(f.outfit, s.outfit),
    }));
  };

  useEffect(() => {
    if (!ready) return;
    canReadPictures()
      .then(setCanSee)
      .catch(() => {
        setCanSee(false);
      });
  }, [ready]);

  /** The picture the model can read: the new upload, or the saved one. */
  const pictureSource =
    avatar.kind === "staged"
      ? { token: avatar.token }
      : avatar.kind === "keep" && saved?.avatar_path
        ? { avatar: saved.avatar_path }
        : null;

  const lookUp = (fromPicture: boolean) => {
    setLookingUp(true);
    setError(null);
    (fromPicture && pictureSource
      ? suggestLooksFromPicture(fields.name, pictureSource)
      : suggestLooks(fields.name, cardText)
    )
      .then(fill(false))
      .catch((e: unknown) => {
        setError(toUiError(e));
      })
      .finally(() => {
        setLookingUp(false);
      });
  };

  // A card from another app has no Looks: fill them once, for the user to check.
  // It is a help, not a step, so a failure shows no error and the button stays.
  useEffect(() => {
    if (autoLooks.current || !draft || !ready) return;
    if (fields.appearance_tags.trim() !== "" && fields.outfit.trim() !== "") return;
    if (cardText.trim() === "") return;
    autoLooks.current = true;
    // The same "Suggesting..." as the button: the chat model is busy
    // meanwhile. Set in the promise, not in the effect body.
    void Promise.resolve()
      .then(() => {
        setLookingUp(true);
        return suggestLooks(fields.name, cardText);
      })
      .then(fill(true))
      .catch(() => undefined)
      .finally(() => {
        setLookingUp(false);
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps -- runs once, when the engine is ready
  }, [ready]);

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
            {(adultOn || fields.is_adult) && (
              <label className="check-confirm">
                <input
                  type="checkbox"
                  checked={fields.is_adult}
                  onChange={(e) => {
                    set("is_adult", e.target.checked);
                  }}
                />
                {strings.creator.adult}
              </label>
            )}
            {fields.is_adult && fields.age > 0 && fields.age < 18 && (
              <p className="field-hint warn">{strings.creator.adultAge}</p>
            )}
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
          <Field
            label={strings.creator.tags}
            hint={needsPersonWord(fields.appearance_tags) ? strings.creator.personWord : strings.creator.tagsHint}
            warn={needsPersonWord(fields.appearance_tags)}
          >
            <input
              type="text"
              maxLength={400}
              value={fields.appearance_tags}
              onChange={(e) => {
                set("appearance_tags", e.target.value);
              }}
            />
          </Field>
          <Field label={strings.creator.outfit} hint={strings.creator.outfitHint}>
            <input
              type="text"
              maxLength={400}
              value={fields.outfit}
              onChange={(e) => {
                set("outfit", e.target.value);
              }}
            />
          </Field>
          <button
            type="button"
            className="link-button"
            disabled={!ready || lookingUp || cardText.trim() === ""}
            onClick={() => {
              lookUp(false);
            }}
          >
            {lookingUp ? strings.creator.suggesting : strings.creator.suggestLooks}
          </button>
          {canSee && pictureSource && (
            <button
              type="button"
              className="link-button"
              disabled={!ready || lookingUp}
              onClick={() => {
                lookUp(true);
              }}
            >
              {lookingUp ? strings.creator.suggesting : strings.creator.suggestLooksPicture}
            </button>
          )}
          {lookedUp && (
            <p className="hint" role="status">
              {strings.creator.lookedUp}
            </p>
          )}
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
