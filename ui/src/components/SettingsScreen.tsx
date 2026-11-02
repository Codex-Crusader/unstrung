import { useEffect, useState } from "react";
import {
  diagnosticReport,
  getNotify,
  getUiSettings,
  listCopies,
  restartSafe,
  setNotify,
  setUiSetting,
  toUiError,
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
