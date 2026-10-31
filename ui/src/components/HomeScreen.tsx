import { useEffect, useRef, useState } from "react";
import {
  importCard,
  listCharacters,
  restoreStarters,
  toUiError,
  type Character,
  type ImportDraft,
  type UiError,
} from "../api";
import { strings } from "../strings";
import { Avatar } from "./Avatar";
import { Icon } from "./Icon";

export function HomeScreen({
  onOpen,
  onCreate,
  onEdit,
  onImport,
}: {
  onOpen: (character: Character) => void;
  onCreate: () => void;
  onEdit: (character: Character) => void;
  onImport: (draft: ImportDraft) => void;
}) {
  const fileRef = useRef<HTMLInputElement>(null);
  const [importing, setImporting] = useState(false);
  const [importError, setImportError] = useState<string | null>(null);

  const importFile = (file: File) => {
    setImportError(null);
    setImporting(true);
    file
      .arrayBuffer()
      .then((buffer) => importCard(new Uint8Array(buffer)))
      .then(onImport)
      .catch((e: unknown) => {
        setImportError(toUiError(e).message);
      })
      .finally(() => {
        setImporting(false);
      });
  };
  const [characters, setCharacters] = useState<Character[] | null>(null);
  const [error, setError] = useState<UiError | null>(null);
  const [note, setNote] = useState<string | null>(null);

  const load = () => {
    listCharacters()
      .then(setCharacters)
      .catch((e: unknown) => {
        setError(toUiError(e));
      });
  };
  useEffect(load, []);

  const restore = () => {
    restoreStarters()
      .then(() => {
        setNote(strings.home.restored);
        load();
      })
      .catch((e: unknown) => {
        setError(toUiError(e));
      });
  };

  return (
    <section className="home">
      <h1>{strings.home.title}</h1>
      {error && (
        <p className="chat-error" role="alert">
          {strings.home.loadFailed} {error.detail}
        </p>
      )}
      <ul className="card-grid">
        <li>
          <button type="button" className="character-card character-card-new" onClick={onCreate}>
            <span className="card-plus" aria-hidden="true">
              <Icon name="plus" />
            </span>
            <span className="card-name">{strings.home.create}</span>
            <span className="card-tagline">{strings.home.createHint}</span>
          </button>
        </li>
        {characters?.map((c) => (
          <li key={c.id} className="card-cell">
            <button
              type="button"
              className="character-card"
              onClick={() => {
                onOpen(c);
              }}
            >
              <Avatar kind="character" owner={c} size="large" />
              <span className="card-name">{c.name}</span>
              <span className="card-tagline">{c.tagline}</span>
            </button>
            <button
              type="button"
              className="card-edit link-button"
              aria-label={strings.home.edit(c.name)}
              onClick={() => {
                onEdit(c);
              }}
            >
              <Icon name="edit" />
              {strings.message.edit}
            </button>
          </li>
        ))}
      </ul>
      {characters?.length === 0 && <p className="muted">{strings.home.empty}</p>}
      <div className="home-import">
        <button
          type="button"
          className="button button-quiet"
          disabled={importing}
          onClick={() => fileRef.current?.click()}
        >
          <Icon name="import" />
          {importing ? strings.home.importing : strings.home.import}
        </button>
        <span className="field-hint">{strings.home.importHint}</span>
        <input
          ref={fileRef}
          type="file"
          accept=".png,.json,image/png,application/json"
          hidden
          onChange={(e) => {
            const file = e.target.files?.[0];
            e.target.value = "";
            if (file) importFile(file);
          }}
        />
      </div>
      {importError && (
        <p className="chat-error" role="alert">
          {importError}
        </p>
      )}
      <p className="home-foot">
        <button type="button" className="link-button" onClick={restore}>
          {strings.home.restore}
        </button>
        {note && <span className="muted"> {note}</span>}
      </p>
    </section>
  );
}
