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

interface Owner {
  id: number;
  name: string;
  avatar_path: string | null;
}

export function Avatar({
  kind,
  owner,
  label,
  size,
}: {
  kind: "character" | "persona";
  owner: Owner | null;
  label?: string;
  size?: "large";
}) {
  const [failedFor, setFailedFor] = useState<string | null>(null);
  const name = label ?? owner?.name ?? "";
  const className = size === "large" ? "avatar avatar-large" : "avatar";
  const path = owner?.avatar_path ?? null;
  if (!owner || path === null || failedFor === path) {
    return (
      <span className={className}>
        <Silhouette kind={kind} label={name} />
      </span>
    );
  }
  return (
    <img
      className={className}
      src={avatarUrl(kind, owner.id, path)}
      alt={name}
      draggable={false}
      onError={() => {
        setFailedFor(path);
      }}
    />
  );
}
