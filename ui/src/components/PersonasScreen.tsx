import { useCallback, useEffect, useState } from "react";
import {
  canReadPictures,
  suggestLooksFromPicture,
  deletePersona,
  listPersonas,
  savePersona,
  setDefaultPersona,
  toUiError,
  type AvatarChange,
  type Persona,
  type PersonaFields,
  type UiError,
} from "../api";
import { estimateTokens, needsPersonWord } from "../cardCheck";
import { strings } from "../strings";
import { Avatar } from "./Avatar";
import { PictureField } from "./PictureField";
import { Icon } from "./Icon";
import { ErrorNote } from "./ErrorNote";

const EMPTY: PersonaFields = { name: "", description: "", appearance_tags: "", outfit: "", age: null };
const DESCRIPTION_TOKENS = 100;

type Editing = { persona: Persona | null; fields: PersonaFields; avatar: AvatarChange };

export function PersonasScreen({ onChanged }: { onChanged: () => void }) {
  const [personas, setPersonas] = useState<Persona[]>([]);
  const [editing, setEditing] = useState<Editing | null>(null);
  const [confirm, setConfirm] = useState(false);
  const [error, setError] = useState<UiError | null>(null);
  const [canSee, setCanSee] = useState(false);
  const [lookingUp, setLookingUp] = useState(false);
  useEffect(() => {
    canReadPictures()
      .then(setCanSee)
      .catch(() => {
        setCanSee(false);
      });
  }, []);

  const load = useCallback(() => {
    listPersonas()
      .then(setPersonas)
      .catch((e: unknown) => {
        setError(toUiError(e));
      });
  }, []);
  useEffect(load, [load]);

  const done = () => {
    setEditing(null);
    setConfirm(false);
    setError(null);
    load();
    onChanged();
  };
  const fail = (e: unknown) => {
    setError(toUiError(e));
  };

  const edit = (persona: Persona | null) => {
    setConfirm(false);
    setError(null);
    setEditing({
      persona,
      fields: persona
        ? {
            name: persona.name,
            description: persona.description,
            appearance_tags: persona.appearance_tags,
            outfit: persona.outfit,
            age: persona.age,
          }
        : EMPTY,
      avatar: { kind: "keep" },
    });
  };

  const hasDefault = personas.some((p) => p.is_default);

  /** The picture the model can read: the new upload, or the saved one. */
  const pictureSource = !editing
    ? null
    : editing.avatar.kind === "staged"
      ? { token: editing.avatar.token }
      : editing.avatar.kind === "keep" && editing.persona?.avatar_path
        ? { avatar: editing.persona.avatar_path }
        : null;

  const lookUp = () => {
    if (!editing || !pictureSource) return;
    setLookingUp(true);
    setError(null);
    suggestLooksFromPicture(editing.fields.name, pictureSource)
      .then((s) => {
        setEditing((e) =>
          e
            ? {
                ...e,
                fields: {
                  ...e.fields,
                  appearance_tags: s.looks || e.fields.appearance_tags,
                  outfit: s.outfit || e.fields.outfit,
                },
              }
            : e,
        );
      })
      .catch(fail)
      .finally(() => {
        setLookingUp(false);
      });
  };

  return (
    <section className="page personas">
      <h1>{strings.personas.title}</h1>
      <p className="muted">{strings.personas.intro}</p>
      {error && <ErrorNote error={error} />}

      {editing ? (
        <form
          className="persona-form"
          onSubmit={(e) => {
            e.preventDefault();
            savePersona(editing.persona?.id ?? null, editing.fields, editing.avatar).then(done).catch(fail);
          }}
        >
          <PictureField
            kind="persona"
            owner={editing.persona}
            value={editing.avatar}
            onChange={(avatar) => {
              setEditing({ ...editing, avatar });
            }}
          />
          <div className="persona-fields">
            <label className="field">
              <span className="field-label">
                {strings.personas.name}
                <span className="field-required"> · {strings.creator.required}</span>
              </span>
              <input
                type="text"
                maxLength={80}
                value={editing.fields.name}
                onChange={(e) => {
                  setEditing({ ...editing, fields: { ...editing.fields, name: e.target.value } });
                }}
              />
            </label>
            <label className="field">
              <span className="field-label">{strings.personas.description}</span>
              <textarea
                rows={3}
                maxLength={600}
                value={editing.fields.description}
                onChange={(e) => {
                  setEditing({ ...editing, fields: { ...editing.fields, description: e.target.value } });
                }}
              />
              <span
                className={
                  estimateTokens(editing.fields.description) > DESCRIPTION_TOKENS ? "field-hint warn" : "field-hint"
                }
              >
                {strings.personas.descriptionHint}{" "}
                {strings.personas.count(estimateTokens(editing.fields.description))}
              </span>
            </label>
            <label className="field">
              <span className="field-label">{strings.personas.tags}</span>
              <input
                type="text"
                maxLength={400}
                value={editing.fields.appearance_tags}
                onChange={(e) => {
                  setEditing({ ...editing, fields: { ...editing.fields, appearance_tags: e.target.value } });
                }}
              />
              {needsPersonWord(editing.fields.appearance_tags) ? (
                <span className="field-hint warn">{strings.creator.personWord}</span>
              ) : (
                <span className="field-hint">{strings.personas.tagsHint}</span>
              )}
            </label>
            <label className="field">
              <span className="field-label">{strings.personas.outfit}</span>
              <input
                type="text"
                maxLength={400}
                value={editing.fields.outfit}
                onChange={(e) => {
                  setEditing({ ...editing, fields: { ...editing.fields, outfit: e.target.value } });
                }}
              />
              <span className="field-hint">{strings.personas.outfitHint}</span>
            </label>
            {canSee && pictureSource && (
              <button type="button" className="link-button" disabled={lookingUp} onClick={lookUp}>
                {lookingUp ? strings.creator.suggesting : strings.creator.suggestLooksPicture}
              </button>
            )}
            <label className="field field-short">
              <span className="field-label">{strings.personas.age}</span>
              <input
                type="number"
                min={1}
                max={10000}
                value={editing.fields.age ?? ""}
                onChange={(e) => {
                  const age = e.target.value === "" ? null : Math.trunc(Number(e.target.value));
                  setEditing({ ...editing, fields: { ...editing.fields, age } });
                }}
              />
            </label>
            <div className="form-actions">
              {editing.persona &&
                (confirm ? (
                  <span className="confirm">
                    <span>{strings.personas.confirmDelete(editing.persona.name)}</span>
                    <button
                      type="button"
                      className="link-button danger"
                      onClick={() => {
                        if (editing.persona) deletePersona(editing.persona.id).then(done).catch(fail);
                      }}
                    >
                      {strings.personas.yes}
                    </button>
                    <button
                      type="button"
                      className="link-button"
                      onClick={() => {
                        setConfirm(false);
                      }}
                    >
                      {strings.personas.no}
                    </button>
                  </span>
                ) : (
                  <button
                    type="button"
                    className="link-button danger"
                    onClick={() => {
                      setConfirm(true);
                    }}
                  >
                    {strings.personas.delete}
                  </button>
                ))}
              <span className="spacer" />
              <button
                type="button"
                className="button button-quiet"
                onClick={() => {
                  setEditing(null);
                  setError(null);
                }}
              >
                {strings.personas.cancel}
              </button>
              <button type="submit" className="button" disabled={editing.fields.name.trim() === ""}>
                {strings.personas.save}
              </button>
            </div>
          </div>
        </form>
      ) : (
        <>
          <ul className="persona-list">
            {personas.map((p) => (
              <li key={p.id} className="persona-row">
                <Avatar kind="persona" owner={p} />
                <div className="persona-info">
                  <span className="card-name">{p.name}</span>
                  <span className="card-tagline">{p.description}</span>
                </div>
                {p.is_default ? (
                  <span className="badge">{strings.personas.isDefault}</span>
                ) : (
                  <button
                    type="button"
                    className="link-button"
                    onClick={() => {
                      setDefaultPersona(p.id).then(done).catch(fail);
                    }}
                  >
                    {strings.personas.default}
                  </button>
                )}
                <button
                  type="button"
                  className="button button-quiet"
                  onClick={() => {
                    edit(p);
                  }}
                >
                  <Icon name="edit" />
                  {strings.personas.edit}
                </button>
              </li>
            ))}
          </ul>
          {personas.length === 0 && <p className="muted">{strings.personas.empty}</p>}
          <div className="form-actions">
            {hasDefault ? (
              <button
                type="button"
                className="link-button"
                onClick={() => {
                  setDefaultPersona(null).then(done).catch(fail);
                }}
              >
                {strings.personas.clearDefault}
              </button>
            ) : (
              <span className="muted">{strings.personas.noDefault}</span>
            )}
            <span className="spacer" />
            <button
              type="button"
              className="button"
              onClick={() => {
                edit(null);
              }}
            >
              <Icon name="plus" />
              {strings.personas.create}
            </button>
          </div>
        </>
      )}
    </section>
  );
}
