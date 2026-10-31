import { useState } from "react";
import { diagnosticReport, startNormally, toUiError, type StartView, type UiError } from "../api";
import figure from "../assets/figure.png";
import { strings } from "../strings";
import { ErrorNote } from "./ErrorNote";
import { Icon } from "./Icon";

/** The one screen of safe start: no engines and no chats, only repair tools. */
export function SafeStart({ start }: { start: StartView }) {
  const s = strings.safe;
  const [copied, setCopied] = useState<boolean | null>(null);
  const [error, setError] = useState<UiError | null>(null);

  const copy = () => {
    diagnosticReport()
      .then((report) => navigator.clipboard.writeText(report))
      .then(() => {
        setCopied(true);
      })
      .catch(() => {
        setCopied(false);
      });
  };

  return (
    <main className="safe-start">
      <div className="safe-card">
        <p className="brand">
          <img className="brand-figure" src={figure} alt="" />
          {strings.appName}
        </p>
        <h1>{s.title}</h1>
        <p>{(start.reason && s.reasons.get(start.reason)) ?? s.reasons.get("asked")}</p>
        {start.error && <ErrorNote error={{ message: s.dataError, detail: start.error }} />}
        <p className="muted">{s.keep}</p>
        <div className="safe-actions">
          <button
            type="button"
            className="button button-big"
            onClick={() => {
              startNormally().catch((e: unknown) => {
                setError(toUiError(e));
              });
            }}
          >
            <Icon name="refresh" />
            {s.startNormally}
          </button>
          <button type="button" className="button button-quiet" onClick={copy}>
            <Icon name="link" />
            {s.copyReport}
          </button>
        </div>
        {copied === true && <p className="field-hint">{s.copied}</p>}
        {copied === false && <p className="field-hint warn">{s.copyFailed}</p>}
        <p className="field-hint">{s.reportHint}</p>
        {error && <ErrorNote error={error} />}
      </div>
    </main>
  );
}
