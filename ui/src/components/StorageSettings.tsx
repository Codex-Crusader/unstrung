// The space that the data takes: where each folder is, what uses the space, and how to free some.
import { useEffect, useState } from "react";
import {
  checkFolder,
  freeUpCandidates,
  freeUpSpace,
  moveFolder,
  pickFolder,
  stopReply,
  storageUsage,
  toUiError,
  type Candidate,
  type FolderCheck,
  type FolderPurpose,
  type UiError,
  type Usage,
} from "../api";
import { strings } from "../strings";
import { ErrorNote } from "./ErrorNote";

export function bytesText(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${String(Math.round(bytes / 1024))} KB`;
}

/** One folder that "Change" moves: pick, check, confirm, copy with progress. */
function FolderRow({
  purpose,
  folder,
  bytes,
  onDone,
}: {
  purpose: FolderPurpose;
  folder: string;
  bytes: number;
  onDone: () => void;
}) {
  const s = strings.settings;
  const [check, setCheck] = useState<FolderCheck | null>(null);
  const [picked, setPicked] = useState("");
  const [copied, setCopied] = useState<[number, number] | null>(null);
  const [stayed, setStayed] = useState<string[]>([]);
  const [error, setError] = useState<UiError | null>(null);
  const issues = (check?.issues ?? []).map((i) => strings.welcome.issues.get(i) ?? i);
  return (
    <div className="folder-row">
      <p>
        {s.folderOf(purpose)}: <span className="folder-path">{folder}</span>{" "}
        {!check && !copied && (
          <button
            type="button"
            className="button button-quiet"
            onClick={() => {
              setError(null);
              pickFolder(folder)
                .then(async (path) => {
                  if (!path) return;
                  setPicked(path);
                  setCheck(await checkFolder(path, purpose, bytes));
                })
                .catch((e: unknown) => {
                  setError(toUiError(e));
                });
            }}
          >
            {s.change}
          </button>
        )}
      </p>
      {check && (
        <div role="alertdialog" aria-label={s.folderOf(purpose)}>
          {issues.map((text) => (
            <p key={text} className="field-hint warn">
              {text}
            </p>
          ))}
          {!check.blocked && <p>{s.moveAsk(purpose, check.folder, bytesText(bytes))}</p>}
          <div className="safe-actions">
            {!check.blocked && (
              <button
                type="button"
                className="button"
                onClick={() => {
                  setCheck(null);
                  setCopied([0, bytes]);
                  moveFolder(purpose, picked, (done, total) => {
                    setCopied([done, total]);
                  })
                    .then((left) => {
                      setCopied(null);
                      setStayed(left);
                      if (left.length === 0) onDone();
                    })
                    .catch((e: unknown) => {
                      setCopied(null);
                      setError(toUiError(e));
                    });
                }}
              >
                {s.moveYes}
              </button>
            )}
            <button
              type="button"
              className="button button-quiet"
              onClick={() => {
                setCheck(null);
              }}
            >
              {strings.safe.cancel}
            </button>
          </div>
        </div>
      )}
      {copied && (
        <div role="status">
          <progress value={copied[0]} max={Math.max(copied[1], 1)} />
          <p className="field-hint">
            {s.moving(bytesText(copied[0]), bytesText(copied[1]))}{" "}
            <button type="button" className="button button-quiet" onClick={() => void stopReply()}>
              {s.stopMove}
            </button>
          </p>
        </div>
      )}
      {stayed.length > 0 && (
        <p className="field-hint warn" role="status">
          {s.oldStayed(stayed.join(", "))}{" "}
          <button
            type="button"
            className="button button-quiet"
            onClick={() => {
              setStayed([]);
              onDone();
            }}
          >
            {s.ok}
          </button>
        </p>
      )}
      {error && <ErrorNote error={error} />}
    </div>
  );
}

/** A bar of disk use by part, the same as a list in words, and the free space. */
export function UsageView({ onChange }: { onChange: () => void }) {
  const s = strings.settings;
  const [usage, setUsage] = useState<Usage | "failed" | null>(null);
  useEffect(() => {
    storageUsage()
      .then(setUsage)
      .catch(() => {
        setUsage("failed");
      });
  }, []);
  if (usage === null) return <p className="field-hint">{s.measuring}</p>;
  if (usage === "failed") return <p className="field-hint warn">{s.measureFailed}</p>;
  const parts = usage.parts.filter((p) => p.bytes > 0);
  return (
    <div className="usage">
      <div className="usage-bar" aria-hidden="true">
        {parts.map((p) => (
          <span key={p.kind} className={`usage-${p.kind}`} style={{ flexGrow: p.bytes }} />
        ))}
      </div>
      <ul className="usage-legend">
        {parts.map((p) => (
          <li key={p.kind}>
            <span className={`usage-dot usage-${p.kind}`} aria-hidden="true" />
            {s.kinds[p.kind]}: {bytesText(p.bytes)}
          </li>
        ))}
      </ul>
      {usage.disks.map((d) => (
        <p key={d.folders.join()} className={d.low ? "field-hint warn" : "field-hint"}>
          {s.free(d.folders, bytesText(d.free))}
          {d.low && ` ${s.lowSpace}`}
        </p>
      ))}
      <FolderRow
        purpose="library"
        folder={usage.library}
        bytes={usage.parts.find((p) => p.kind === "gallery")?.bytes ?? 0}
        onDone={onChange}
      />
      <FolderRow
        purpose="models"
        folder={usage.models}
        bytes={usage.parts
          .filter((p) => p.kind === "chat_models" || p.kind === "picture_models")
          .reduce((n, p) => n + p.bytes, 0)}
        onDone={onChange}
      />
    </div>
  );
}

/** "Free up space": a list with sizes, then a confirm with the total. */
export function FreeUp({ onDone }: { onDone: () => void }) {
  const s = strings.settings;
  const [list, setList] = useState<Candidate[] | null>(null);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [confirm, setConfirm] = useState(false);
  const [freed, setFreed] = useState<number | null>(null);
  const [error, setError] = useState<UiError | null>(null);
  const total = (list ?? []).filter((c) => picked.has(c.id)).reduce((n, c) => n + c.bytes, 0);
  if (list === null) {
    return (
      <>
        <button
          type="button"
          className="button button-quiet"
          onClick={() => {
            setFreed(null);
            setError(null);
            freeUpCandidates()
              .then((found) => {
                setList(found);
                setPicked(new Set(found.filter((c) => c.checked).map((c) => c.id)));
              })
              .catch((e: unknown) => {
                setError(toUiError(e));
              });
          }}
        >
          {s.freeUp}
        </button>
        {freed !== null && (
          <p className="field-hint" role="status">
            {s.freed(bytesText(freed))}
          </p>
        )}
        {error && <ErrorNote error={error} />}
      </>
    );
  }
  return (
    <div className="free-up" role="group" aria-label={s.freeUp}>
      {list.length === 0 && <p>{s.nothingToFree}</p>}
      {list.map((c) => (
        <div key={c.id} className="free-up-item">
          <label className="check-confirm">
            <input
              type="checkbox"
              checked={picked.has(c.id)}
              onChange={(e) => {
                const next = new Set(picked);
                if (e.target.checked) next.add(c.id);
                else next.delete(c.id);
                setPicked(next);
                setConfirm(false);
              }}
            />
            {s.candidates[c.id]}: {bytesText(c.bytes)}
          </label>
          {(c.items.length > 0 || c.hidden > 0) && (
            <details>
              <summary>{s.view}</summary>
              <ul>
                {c.items.map((item) => (
                  <li key={item}>{item}</li>
                ))}
                {c.hidden > 0 && <li>{s.hiddenChats(c.hidden)}</li>}
              </ul>
            </details>
          )}
        </div>
      ))}
      <p className="field-hint">{s.modelsHint}</p>
      {confirm ? (
        <div role="alertdialog" aria-label={s.freeUp}>
          <p>{s.freeAsk(bytesText(total))}</p>
          <div className="safe-actions">
            <button
              type="button"
              className="button danger"
              onClick={() => {
                freeUpSpace([...picked])
                  .then((n) => {
                    setFreed(n);
                    setList(null);
                    setConfirm(false);
                    onDone();
                  })
                  .catch((e: unknown) => {
                    setConfirm(false);
                    setError(toUiError(e));
                  });
              }}
            >
              {s.freeYes}
            </button>
            <button
              type="button"
              className="button button-quiet"
              onClick={() => {
                setConfirm(false);
              }}
            >
              {strings.safe.cancel}
            </button>
          </div>
        </div>
      ) : (
        <div className="safe-actions">
          <button
            type="button"
            className="button"
            disabled={picked.size === 0}
            onClick={() => {
              setConfirm(true);
            }}
          >
            {s.freeButton(bytesText(total))}
          </button>
          <button
            type="button"
            className="button button-quiet"
            onClick={() => {
              setList(null);
            }}
          >
            {strings.safe.cancel}
          </button>
        </div>
      )}
      {error && <ErrorNote error={error} />}
    </div>
  );
}
