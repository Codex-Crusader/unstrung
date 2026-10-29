import { useEffect, useState } from "react";
import { listCharacters, toUiError, type Character, type UiError } from "../api";
import { strings } from "../strings";
import { Avatar } from "./Avatar";

export function HomeScreen({
  onOpen,
  onCreate,
  onEdit,
}: {
  onOpen: (character: Character) => void;
  onCreate: () => void;
  onEdit: (character: Character) => void;
}) {
  const [characters, setCharacters] = useState<Character[] | null>(null);
  const [error, setError] = useState<UiError | null>(null);

  useEffect(() => {
    listCharacters()
      .then(setCharacters)
      .catch((e: unknown) => {
        setError(toUiError(e));
      });
  }, []);

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
              +
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
              {strings.message.edit}
            </button>
          </li>
        ))}
      </ul>
      {characters?.length === 0 && <p className="muted">{strings.home.empty}</p>}
    </section>
  );
}
