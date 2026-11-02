import { useEffect, useState } from "react";
import {
  addFromBackup,
  diagnosticReport,
  discardBackup,
  exportBackup,
  getNotify,
  getUiSettings,
  listCopies,
  openBackup,
  replaceFromBackup,
  restartSafe,
  setNotify,
  setUiSetting,
  toUiError,
  type Added,
  type BackupInfo,
  type Exported,
  type UiError,
} from "../api";
import { applyAppearance, type UiSettings } from "../appearance";
import { strings } from "../strings";
import { ErrorNote } from "./ErrorNote";

/** The Windows notification when a download ends in the background. */
function NotifyToggle() {
  const [on, setOn] = useState<boolean | null>(null);
  useEffect(() => {
    void getNotify().then(setOn);
  }, []);
  if (on === null) return null;
  return (
    <label className="check-confirm">
      <input
        type="checkbox"
        checked={on}
        onChange={(e) => {
          const next = e.target.checked;
          setOn(next);
          void setNotify(next);
        }}
      />
      {strings.welcome.notifyLabel}
    </label>
  );
}

/** One setting as a row of choices. */
function Choices({
  label,
  name,
  value,
  options,
  onPick,
}: {
  label: string;
  name: string;
  value: string;
  options: [string, string][];
  onPick: (value: string) => void;
}) {
  return (
    <fieldset className="choices">
      <legend className="field-label">{label}</legend>
      {options.map(([v, text]) => (
        <label key={v} className="choice">
          <input
            type="radio"
            name={name}
            checked={value === v}
            onChange={() => {
              onPick(v);
            }}
          />
          {text}
        </label>
      ))}
    </fieldset>
  );
}

/** Export everything and Import backup. */
function Storage() {
  const s = strings.settings;
  const [state, setState] = useState<"idle" | "working" | Exported>("idle");
  const [importing, setImporting] = useState<"idle" | "working" | BackupInfo | Added>("idle");
  const [error, setError] = useState<UiError | null>(null);
  const [confirmReplace, setConfirmReplace] = useState(false);
  const fail = (e: unknown) => {
    setImporting("idle");
    setError(toUiError(e));
  };
  const opened = typeof importing === "object" && "id" in importing ? importing : null;
  return (
    <section className="settings-group" aria-label={s.storage}>
      <h2>{s.storage}</h2>
      <p>{s.backupIntro}</p>
      <div className="safe-actions">
        <button
          type="button"
          className="button button-quiet"
          disabled={state === "working"}
          onClick={() => {
            setState("working");
            setError(null);
            exportBackup()
              .then((done) => {
                setState(done ?? "idle");
              })
              .catch((e: unknown) => {
                setState("idle");
                setError(toUiError(e));
              });
          }}
        >
          {state === "working" ? s.exporting : s.exportAll}
        </button>
        {opened === null && (
          <button
            type="button"
            className="button button-quiet"
            disabled={importing === "working"}
            onClick={() => {
              setImporting("working");
              setError(null);
              openBackup()
                .then((info) => {
                  setImporting(info ?? "idle");
                })
                .catch(fail);
            }}
          >
            {importing === "working" ? s.importing : s.importBackup}
          </button>
        )}
      </div>
      {typeof state === "object" && (
        <p className="field-hint" role="status">
          {s.exported(state.pictures, state.bytes)}
        </p>
      )}
      {opened !== null && (
        <div role="group" aria-label={s.importBackup}>
          <p>{s.backupHolds(opened)}</p>
          <div className="safe-actions">
            <button
              type="button"
              className="button"
              onClick={() => {
                setImporting("working");
                addFromBackup(opened.id).then(setImporting).catch(fail);
              }}
            >
              {s.addOnly}
            </button>
            <button
              type="button"
              className="button button-quiet"
              onClick={() => {
                setConfirmReplace(true);
              }}
            >
              {s.replace}
            </button>
            <button
              type="button"
              className="button button-quiet"
              onClick={() => {
                setImporting("idle");
                setConfirmReplace(false);
                void discardBackup(opened.id).catch(() => undefined);
              }}
            >
              {strings.safe.cancel}
            </button>
          </div>
          <p className="field-hint">{s.addOnlyHint}</p>
          {confirmReplace && (
            <div role="alertdialog" aria-label={s.replace}>
              <p>{s.replaceAsk}</p>
              <div className="safe-actions">
                <button
                  type="button"
                  className="button danger"
                  onClick={() => {
                    replaceFromBackup(opened.id).catch((e: unknown) => {
                      setConfirmReplace(false);
                      setError(toUiError(e));
                    });
                  }}
                >
                  {s.replaceYes}
                </button>
                <button
                  type="button"
                  className="button button-quiet"
                  onClick={() => {
                    setConfirmReplace(false);
                  }}
                >
                  {strings.safe.cancel}
                </button>
              </div>
            </div>
          )}
        </div>
      )}
      {typeof importing === "object" && "skipped" in importing && (
        <p className="field-hint" role="status">
          {s.added(importing)}
        </p>
      )}
      {error && <ErrorNote error={error} />}
    </section>
  );
}

export function SettingsScreen() {
  const s = strings.settings;
  const [ui, setUi] = useState<UiSettings | null>(null);
  const [copies, setCopies] = useState<number | null>(null);
  const [askRestart, setAskRestart] = useState(false);
  const [copied, setCopied] = useState<boolean | null>(null);
  const [error, setError] = useState<UiError | null>(null);
  useEffect(() => {
    getUiSettings()
      .then(setUi)
      .catch((e: unknown) => {
        setError(toUiError(e));
      });
    listCopies()
      .then((list) => {
        setCopies(list.length);
      })
      .catch(() => {
        setCopies(null);
      });
  }, []);

  const pick = (key: string) => (value: string) => {
    if (!ui) return;
    const next = { ...ui, [key]: value };
    setUi(next);
    applyAppearance(next);
    setUiSetting(key, value).catch((e: unknown) => {
      setError(toUiError(e));
    });
  };

  return (
    <section className="page settings">
      <h1>{s.title}</h1>
      {ui && (
        <section className="settings-group" aria-label={s.appearance}>
          <h2>{s.appearance}</h2>
          <Choices
            label={s.theme}
            name="theme"
            value={ui.ui_theme ?? "system"}
            options={s.themes}
            onPick={pick("ui_theme")}
          />
          <Choices
            label={s.textSize}
            name="text-size"
            value={ui.ui_text_size ?? "normal"}
            options={s.textSizes}
            onPick={pick("ui_text_size")}
          />
          <label className="check-confirm">
            <input
              type="checkbox"
              checked={ui.ui_motion === "reduce"}
              onChange={(e) => {
                pick("ui_motion")(e.target.checked ? "reduce" : "system");
              }}
            />
            {s.reduceMotion}
          </label>
          <p className="field-hint">{s.reduceMotionHint}</p>
        </section>
      )}

      <section className="settings-group" aria-label={s.notifications}>
        <h2>{s.notifications}</h2>
        <NotifyToggle />
      </section>

      <section className="settings-group" aria-label={s.safety}>
        <h2>{s.safety}</h2>
        <p>{copies === null ? s.copiesUnknown : s.copies(copies)}</p>
        {!askRestart ? (
          <button
            type="button"
            className="button button-quiet"
            onClick={() => {
              setAskRestart(true);
            }}
          >
            {s.restartSafe}
          </button>
        ) : (
          <div role="alertdialog" aria-label={s.restartSafe}>
            <p>{s.restartAsk}</p>
            <div className="safe-actions">
              <button
                type="button"
                className="button"
                onClick={() => {
                  restartSafe().catch((e: unknown) => {
                    setError(toUiError(e));
                  });
                }}
              >
                {s.restartYes}
              </button>
              <button
                type="button"
                className="button button-quiet"
                onClick={() => {
                  setAskRestart(false);
                }}
              >
                {strings.safe.cancel}
              </button>
            </div>
          </div>
        )}
      </section>

      <Storage />

      <section className="settings-group" aria-label={s.help}>
        <h2>{s.help}</h2>
        <button
          type="button"
          className="button button-quiet"
          onClick={() => {
            diagnosticReport()
              .then((report) => navigator.clipboard.writeText(report))
              .then(() => {
                setCopied(true);
              })
              .catch(() => {
                setCopied(false);
              });
          }}
        >
          {strings.safe.copyReport}
        </button>
        {copied === true && <p className="field-hint">{strings.safe.copied}</p>}
        {copied === false && <p className="field-hint warn">{strings.safe.copyFailed}</p>}
        <p className="field-hint">{strings.safe.reportHint}</p>
      </section>
      {error && <ErrorNote error={error} />}
    </section>
  );
}
