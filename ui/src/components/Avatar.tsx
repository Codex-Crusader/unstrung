import { useState } from "react";
import { avatarUrl } from "../api";

/** The default silhouette: a mannequin bust in theme colors. Filled for characters, outlined for personas. */
export function Silhouette({ kind, label }: { kind: "character" | "persona"; label: string }) {
  return (
    <svg className={`silhouette silhouette-${kind}`} viewBox="0 0 64 64" role="img" aria-label={label}>
      <ellipse cx="32" cy="22" rx="10" ry="12" />
      <path d="M14 58c0-12 8-20 18-20s18 8 18 20z" />
      <circle className="pin" cx="22" cy="44" r="2" />
      <circle className="pin" cx="42" cy="44" r="2" />
    </svg>
  );
}

export function CharacterAvatar({ id, name, hasPicture }: { id: number; name: string; hasPicture: boolean }) {
  const [failed, setFailed] = useState(false);
  if (!hasPicture || failed) {
    return (
      <span className="avatar">
        <Silhouette kind="character" label={name} />
      </span>
    );
  }
  return (
    <img
      className="avatar"
      src={avatarUrl(id)}
      alt={name}
      draggable={false}
      onError={() => {
        setFailed(true);
      }}
    />
  );
}

export function UserAvatar({ label }: { label: string }) {
  return (
    <span className="avatar">
      <Silhouette kind="persona" label={label} />
    </span>
  );
}
