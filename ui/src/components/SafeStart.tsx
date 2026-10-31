import { useEffect, useState } from "react";
import {
  diagnosticReport,
  selfTestMode,
  selfTestReport,
  startNormally,
  toUiError,
  type StartView,
  type UiError,
} from "../api";
import figure from "../assets/figure.png";
import { strings } from "../strings";
import { ErrorNote } from "./ErrorNote";
import { Icon } from "./Icon";

/** The one screen of safe start: no engines and no chats, only repair tools. */
export function SafeStart({ start }: { start: StartView }) {
  const s = strings.safe;
  const [copied, setCopied] = useState<boolean | null>(null);
  const [error, setError] = useState<UiError | null>(null);
  useSafeSelfTest();

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

/**
 * `--self-test --safe-start`: checks the report, then presses "Start
 * normally". The new process runs the normal self-test, which writes the
 * report file. The clipboard is not tested, so a test run never overwrites
 * the user's clipboard.
 */
function useSafeSelfTest() {
  useEffect(() => {
    const fail = (detail: string) =>
      selfTestReport({ ok: false, detail, messages_at_open: 0, messages_after: 0, text_events: 0 });
    void selfTestMode().then(async (on) => {
      if (!on) return;
      try {
        const report = await diagnosticReport();
        if (!report.startsWith("Unstrung ")) return await fail("the report has no version line");
        if (!report.includes("%USERPROFILE%")) return await fail("the report does not mask the user folder");
        if (/[A-Z]:[\\/]Users[\\/]/i.test(report)) return await fail("the report shows a user folder");
        await startNormally();
      } catch (e) {
        await fail(`safe start: ${toUiError(e).message}`);
      }
    });
  }, []);
}
