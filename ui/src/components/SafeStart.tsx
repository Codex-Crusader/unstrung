import { useEffect, useState } from "react";
import {
  diagnosticReport,
  repairCheck,
  repairFix,
  selfTestMode,
  selfTestReport,
  startNormally,
  toUiError,
  type Finding,
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
        <QuickRepair />
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
        const found = await repairCheck();
        const areas = ["engine", "engine_files", "data", "location", "models", "hardware", "temp_files"];
        if (found.map((f) => f.area).join() !== areas.join())
          return await fail(`quick repair returned ${found.map((f) => f.area).join()}`);
        if (found.find((f) => f.area === "data")?.state !== "ok") return await fail("quick repair: the test data failed");
        const fixed = await repairFix();
        if (fixed.some((f) => f.state === "can_fix" || f.state === "needs_action"))
          return await fail(`quick repair left a problem: ${JSON.stringify(fixed)}`);
        await startNormally();
      } catch (e) {
        await fail(`safe start: ${toUiError(e).message}`);
      }
    });
  }, []);
}

function size(bytes: number): string {
  if (bytes < 1024 * 1024) return `${String(Math.max(1, Math.round(bytes / 1024)))} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/** Checks first, shows the result, and fixes only after the user presses Fix. */
function QuickRepair() {
  const s = strings.safe;
  const [busy, setBusy] = useState<"checking" | "fixing" | null>(null);
  const [findings, setFindings] = useState<Finding[] | null>(null);
  const [error, setError] = useState<UiError | null>(null);
  const run = (step: "checking" | "fixing") => {
    setBusy(step);
    setError(null);
    (step === "checking" ? repairCheck() : repairFix())
      .then(setFindings)
      .catch((e: unknown) => {
        setError(toUiError(e));
      })
      .finally(() => {
        setBusy(null);
      });
  };
  const fixable = findings?.filter((f) => f.state === "can_fix").length ?? 0;
  const problems = findings?.filter((f) => f.state !== "ok").length ?? 0;
  const done = findings?.some((f) => f.state === "fixed") ?? false;

  return (
    <section className="quick-repair" aria-label={s.repair}>
      <div className="safe-actions">
        <button
          type="button"
          className="button button-quiet"
          disabled={busy !== null}
          onClick={() => {
            run("checking");
          }}
        >
          <Icon name="check" />
          {busy === "checking" ? s.checking : s.repair}
        </button>
        {fixable > 0 && (
          <button
            type="button"
            className="button"
            disabled={busy !== null}
            onClick={() => {
              run("fixing");
            }}
          >
            {busy === "fixing" ? s.fixing : s.fix(fixable)}
          </button>
        )}
      </div>
      {findings === null && <p className="field-hint">{s.repairHint}</p>}
      {findings && (
        <ul className="repair-list">
          {findings.map((f) => (
            <li key={f.area} className={`repair-${f.state}`}>
              <Icon name={f.state === "ok" || f.state === "fixed" ? "check" : "warning"} />
              <span>{s.findings.get(`${f.area}.${f.state}`)?.(size(f.bytes)) ?? `${f.area}: ${f.state}`}</span>
              {f.detail && (
                <details className="repair-detail">
                  <summary>{strings.status.details}</summary>
                  <code>{f.detail}</code>
                </details>
              )}
            </li>
          ))}
        </ul>
      )}
      {findings && problems === 0 && <p className="field-hint">{s.noProblems}</p>}
      {findings && done && fixable === 0 && <p className="field-hint">{s.allFixed}</p>}
      {error && <ErrorNote error={error} />}
    </section>
  );
}
