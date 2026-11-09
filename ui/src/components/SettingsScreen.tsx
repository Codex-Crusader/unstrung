import { useEffect, useState } from "react";
import {
  addFromBackup,
  autoBackupStatus,
  backupStatus,
  chooseAutoBackupFolder,
  stopAutoBackup,
  type AutoBackup,
  checkFolder,
  diagnosticReport,
  discardBackup,
  exportBackup,
  freeUpCandidates,
  freeUpSpace,
  getAdultContent,
  getNotify,
  getUiSettings,
  listCopies,
  moveFolder,
  openBackup,
  pickFolder,
  replaceFromBackup,
  restartSafe,
  setAdultContent,
  setNotify,
  setUiSetting,
  stopReply,
  storageUsage,
  toUiError,
  type Added,
  type BackupInfo,
  type Candidate,
  type FolderCheck,
  type FolderPurpose,
  type Exported,
  type UiError,
  type Usage,
} from "../api";
import { applyAppearance, type UiSettings } from "../appearance";
import { strings } from "../strings";
import { ErrorNote } from "./ErrorNote";
import { LicensesView } from "./LicensesView";
import { UpdateSettings } from "./Updates";

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
/** Settings > Content: adult content, off at install. On asks for 18+ once. */
function AdultToggle() {
  const s = strings.settings;
  const [on, setOn] = useState<boolean | null>(null);
  const [asking, setAsking] = useState(false);
  const [error, setError] = useState<UiError | null>(null);
  useEffect(() => {
    getAdultContent()
      .then(setOn)
      .catch((e: unknown) => {
        setError(toUiError(e));
      });
  }, []);
  const save = (next: boolean) => {
    setAsking(false);
    setAdultContent(next)
      .then(() => {
        setOn(next);
      })
      .catch((e: unknown) => {
        setError(toUiError(e));
      });
  };
  if (on === null) return error ? <ErrorNote error={error} /> : null;
  return (
    <>
      <label className="check-confirm">
        <input
          type="checkbox"
          checked={on}
          onChange={(e) => {
            if (e.target.checked) setAsking(true);
            else save(false);
          }}
        />
        {s.adultLabel}
      </label>
      <p className="field-hint">{s.adultHint}</p>
      {asking && (
        <div role="alertdialog" aria-label={s.adultAsk}>
          <p>{s.adultAsk}</p>
          <div className="safe-actions">
            <button
              type="button"
              className="button"
              onClick={() => {
                save(true);
              }}
            >
              {s.adultYes}
            </button>
            <button
              type="button"
              className="button button-quiet"
              onClick={() => {
                setAsking(false);
              }}
            >
              {s.adultNo}
            </button>
          </div>
        </div>
      )}
      {error && <ErrorNote error={error} />}
    </>
  );
}

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
function UsageView({ onChange }: { onChange: () => void }) {
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
function FreeUp({ onDone }: { onDone: () => void }) {
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

/** Export everything and Import backup. */
/** Settings > Storage: the weekly backup into a folder that the user picks. */
function AutoBackupView() {
  const s = strings.settings;
  const [auto, setAuto] = useState<AutoBackup | null>(null);
  const [error, setError] = useState<UiError | null>(null);
  useEffect(() => {
    autoBackupStatus().then(setAuto, () => {});
  }, []);
  if (auto === null) return null;
  const choose = () => {
    setError(null);
    chooseAutoBackupFolder()
      .then((next) => {
        if (next) setAuto(next);
      })
      .catch((e: unknown) => {
        setError(toUiError(e));
      });
  };
  return (
    <div role="group" aria-label={s.autoTitle}>
      <h3>{s.autoTitle}</h3>
      <p>{auto.folder === null ? s.autoOff : s.autoOn(auto.folder, auto.last_ms)}</p>
      {auto.folder !== null && auto.error !== null && <p className="field-hint">{s.autoFailed(auto.error)}</p>}
      <div className="safe-actions">
        <button type="button" className="button button-quiet" onClick={choose}>
          {auto.folder === null ? s.autoChoose : s.autoChange}
        </button>
        {auto.folder !== null && (
          <button
            type="button"
            className="button button-quiet"
            onClick={() => {
              stopAutoBackup().then(setAuto, (e: unknown) => {
                setError(toUiError(e));
              });
            }}
          >
            {s.autoStop}
          </button>
        )}
      </div>
      {error && <ErrorNote error={error} />}
    </div>
  );
}

function Storage() {
  const s = strings.settings;
  const [state, setState] = useState<"idle" | "working" | Exported>("idle");
  const [importing, setImporting] = useState<"idle" | "working" | BackupInfo | Added>("idle");
  const [error, setError] = useState<UiError | null>(null);
  const [confirmReplace, setConfirmReplace] = useState(false);
  // Measures again after "Free up space".
  const [measured, setMeasured] = useState(0);
  const [lastBackup, setLastBackup] = useState<number | null | undefined>(undefined);
  useEffect(() => {
    backupStatus()
      .then((b) => {
        setLastBackup(b.last_ms);
      })
      .catch(() => {});
  }, [state]);
  const fail = (e: unknown) => {
    setImporting("idle");
    setError(toUiError(e));
  };
  const opened = typeof importing === "object" && "id" in importing ? importing : null;
  return (
    <section className="settings-group" aria-label={s.storage}>
      <h2>{s.storage}</h2>
      <UsageView
        key={measured}
        onChange={() => {
          setMeasured((n) => n + 1);
        }}
      />
      <FreeUp
        onDone={() => {
          setMeasured((n) => n + 1);
        }}
      />
      <p>
        {s.backupIntro}
        {lastBackup !== undefined && ` ${s.lastBackup(lastBackup)}`}
      </p>
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
      <AutoBackupView />
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

      <section className="settings-group" aria-label={s.content}>
        <h2>{s.content}</h2>
        <AdultToggle />
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
        <UpdateSettings />
        <LicensesView />
      </section>
      {error && <ErrorNote error={error} />}
    </section>
  );
}
