import { useEffect, useRef, useState } from "react";
import {
  importCard,
  listCharacters,
  recentChats,
  restoreStarters,
  toUiError,
  type Character,
  type ImportDraft,
  type RecentChat,
  type UiError,
} from "../api";
import { strings } from "../strings";
import { Avatar } from "./Avatar";
import { Icon } from "./Icon";
import { ErrorNote } from "./ErrorNote";

export function HomeScreen({
  onOpen,
  onOpenChat,
  onCreate,
  onEdit,
  onImport,
}: {
  onOpen: (character: Character) => void;
  onOpenChat: (characterId: number, chatId: number) => void;
  onCreate: () => void;
  onEdit: (character: Character) => void;
  onImport: (draft: ImportDraft) => void;
}) {
  const fileRef = useRef<HTMLInputElement>(null);
  const [importing, setImporting] = useState(false);
  const [importError, setImportError] = useState<UiError | null>(null);

  const importFile = (file: File) => {
    setImportError(null);
    setImporting(true);
    file
      .arrayBuffer()
      .then((buffer) => importCard(new Uint8Array(buffer)))
      .then(onImport)
      .catch((e: unknown) => {
        setImportError(toUiError(e));
      })
      .finally(() => {
        setImporting(false);
      });
  };
  const [characters, setCharacters] = useState<Character[] | null>(null);
  const [recent, setRecent] = useState<RecentChat[]>([]);
  const [query, setQuery] = useState("");
  const [error, setError] = useState<UiError | null>(null);
  const [note, setNote] = useState<string | null>(null);

  const load = () => {
    listCharacters()
      .then(setCharacters)
      .catch((e: unknown) => {
        setError(toUiError(e));
      });
    // The row is a shortcut: if it cannot load, Home still works.
    recentChats()
      .then(setRecent)
      .catch(() => {
        setRecent([]);
      });
  };
  const wanted = query.trim().toLowerCase();
  const shown = wanted
    ? characters?.filter((c) => `${c.name} ${c.tagline}`.toLowerCase().includes(wanted))
    : characters;
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
      {recent.length > 0 && (
        <section className="recent" aria-labelledby="recent-title">
          <h2 id="recent-title">{strings.home.recent}</h2>
          <ul className="recent-row">
            {recent.map((r) => (
              <li key={r.chat_id}>
                <button
                  type="button"
                  className="recent-chat"
                  onClick={() => {
                    onOpenChat(r.character_id, r.chat_id);
                  }}
                >
                  <Avatar
                    kind="character"
                    owner={{ id: r.character_id, name: r.character_name, avatar_path: r.avatar_path }}
                  />
                  <span className="recent-text">
                    <span className="recent-name">{r.character_name}</span>
                    <span className="recent-preview">{r.preview}</span>
                    <span className="field-hint">{strings.home.when(r.updated_at)}</span>
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}
      <div className="home-head">
        <h1>{strings.home.title}</h1>
        <label className="home-search">
          <Icon name="search" />
          <span className="sr-only">{strings.home.search}</span>
          <input
            type="search"
            value={query}
            placeholder={strings.home.search}
            onChange={(e) => {
              setQuery(e.target.value);
            }}
          />
        </label>
      </div>
      {error && <ErrorNote error={error} lead={strings.home.loadFailed} />}
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
        {shown?.map((c) => (
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
      {wanted && characters?.length !== 0 && shown?.length === 0 && (
        <p className="muted">{strings.home.noMatch(query.trim())}</p>
      )}
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
      {importError && <ErrorNote error={importError} />}
      <p className="home-foot">
        <button type="button" className="link-button" onClick={restore}>
          {strings.home.restore}
        </button>
        {note && <span className="muted"> {note}</span>}
      </p>
    </section>
  );
}
