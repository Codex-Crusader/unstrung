import { useEffect, useState } from "react";
import {
  checkUpdate,
  getUpdateCheck,
  installUpdate,
  onUpdateAvailable,
  onUpdateProgress,
  setUpdateCheck,
  toUiError,
  type UiError,
  type UpdateInfo,
} from "../api";
import { strings } from "../strings";
import { bytesText } from "./StorageSettings";
import { ErrorNote } from "./ErrorNote";

/**
 * The Update button. It downloads and installs only on a click, then the app
 * closes for the installer and opens the new version.
 */
function InstallButton({ update }: { update: UpdateInfo }) {
  const s = strings.updates;
  const [progress, setProgress] = useState<[number, number | null] | null>(null);
  const [error, setError] = useState<UiError | null>(null);
  useEffect(() => {
    const off = onUpdateProgress(setProgress);
    return () => {
      void off.then((f) => {
        f();
      });
    };
  }, []);
  const busy = progress !== null && error === null;
  return (
    <>
      <button
        type="button"
        className="button"
        disabled={busy}
        onClick={() => {
          setError(null);
          setProgress([0, null]);
          installUpdate().catch((e: unknown) => {
            setError(toUiError(e));
          });
        }}
      >
        {busy ? s.downloading(bytesText(progress[0]), progress[1] === null ? null : bytesText(progress[1])) : s.install(update.version)}
      </button>
      {error && <ErrorNote error={error} />}
    </>
  );
}

/** Shown above the status bar when the check after start finds an update. */
export function UpdateBar() {
  const s = strings.updates;
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  useEffect(() => {
    const off = onUpdateAvailable(setUpdate);
    return () => {
      void off.then((f) => {
        f();
      });
    };
  }, []);
  if (!update) return null;
  return (
    <div className="update-bar" role="status">
      <span>{s.ready(update.version)}</span>
      <InstallButton update={update} />
      <button
        type="button"
        className="button button-quiet"
        onClick={() => {
          setUpdate(null);
        }}
      >
        {s.later}
      </button>
    </div>
  );
}

/** Settings > Help: the check setting, and a check on demand. */
export function UpdateSettings() {
  const s = strings.updates;
  const [on, setOn] = useState<boolean | null>(null);
  const [found, setFound] = useState<UpdateInfo | null | undefined>(undefined);
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState<UiError | null>(null);
  useEffect(() => {
    void getUpdateCheck().then(setOn);
  }, []);
  return (
    <div className="update-settings">
      {on !== null && (
        <label className="check-confirm">
          <input
            type="checkbox"
            checked={on}
            onChange={(e) => {
              const next = e.target.checked;
              setOn(next);
              void setUpdateCheck(next);
            }}
          />
          {s.checkLabel}
        </label>
      )}
      <p className="field-hint">{s.checkHint}</p>
      <button
        type="button"
        className="button button-quiet"
        disabled={checking}
        onClick={() => {
          setChecking(true);
          setError(null);
          checkUpdate()
            .then(setFound)
            .catch((e: unknown) => {
              setError(toUiError(e));
            })
            .finally(() => {
              setChecking(false);
            });
        }}
      >
        {checking ? s.checking : s.checkNow}
      </button>
      {found === null && <p className="field-hint">{s.upToDate}</p>}
      {found && (
        <>
          <p className="field-hint">{s.ready(found.version)}</p>
          {found.notes && <p className="field-hint update-notes">{found.notes}</p>}
          <InstallButton update={found} />
        </>
      )}
      {error && <ErrorNote error={error} />}
    </div>
  );
}
