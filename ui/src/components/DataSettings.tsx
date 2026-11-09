// The "Your data" section of Settings: storage, chat numbers, backups and their password.
import { useEffect, useState } from "react";
import {
  addFromBackup,
  autoBackupStatus,
  backupStatus,
  chatStats,
  setBackupPassword,
  chooseAutoBackupFolder,
  stopAutoBackup,
  type AutoBackup,
  type CharacterStats,
  discardBackup,
  exportBackup,
  openBackup,
  replaceFromBackup,
  toUiError,
  type Added,
  type BackupInfo,
  type Exported,
  type UiError,
} from "../api";
import { strings } from "../strings";
import { ErrorNote } from "./ErrorNote";
import { FreeUp, UsageView } from "./StorageSettings";

/** Export everything and Import backup. */
/** Settings > Storage: chats, messages and pictures per character. */
function StatsView() {
  const s = strings.settings;
  const [stats, setStats] = useState<CharacterStats[] | null>(null);
  useEffect(() => {
    chatStats().then(setStats, () => {});
  }, []);
  if (stats === null || stats.length === 0) return null;
  const sum = (pick: (c: CharacterStats) => number) => stats.reduce((n, c) => n + pick(c), 0);
  return (
    <details className="stats">
      <summary>{s.statsTitle}</summary>
      <p>{s.statsTotal(
          sum((c) => c.chats),
          sum((c) => c.messages),
          sum((c) => c.pictures),
        )}</p>
      <table>
        <thead>
          <tr>
            <th scope="col">{s.statsCharacter}</th>
            <th scope="col">{s.statsChats}</th>
            <th scope="col">{s.statsMessages}</th>
            <th scope="col">{s.statsPictures}</th>
          </tr>
        </thead>
        <tbody>
          {stats.map((c) => (
            <tr key={c.character_id}>
              <th scope="row">{c.name}</th>
              <td>{c.chats}</td>
              <td>{c.messages.toLocaleString()}</td>
              <td>{c.pictures}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </details>
  );
}

/** Settings > Storage: set, change or remove the backup password. */
function PasswordView({ sealed, onChange }: { sealed: boolean; onChange: () => void }) {
  const s = strings.settings;
  const [editing, setEditing] = useState(false);
  const [first, setFirst] = useState("");
  const [again, setAgain] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const [error, setError] = useState<UiError | null>(null);
  const save = (password: string | null) => {
    setError(null);
    setBackupPassword(password)
      .then(() => {
        setEditing(false);
        setFirst("");
        setAgain("");
        setNote(password === null ? s.passwordRemoved : s.passwordSaved);
        onChange();
      })
      .catch((e: unknown) => {
        setError(toUiError(e));
      });
  };
  // Code points, as Rust counts chars.
  const problem = Array.from(first).length < 8 ? s.passwordShort : first !== again ? s.passwordDiffer : null;
  return (
    <div role="group" aria-label={s.passwordNew}>
      <p>{sealed ? s.backupSealed : s.backupPlain}</p>
      {editing ? (
        <form
          className="password-form"
          onSubmit={(e) => {
            e.preventDefault();
            if (problem === null) save(first);
          }}
        >
          <label className="field">
            <span className="field-label">{s.passwordNew}</span>
            <input
              type="password"
              autoComplete="new-password"
              value={first}
              onChange={(e) => {
                setFirst(e.target.value);
              }}
            />
            <span className="field-hint">{s.passwordHint}</span>
          </label>
          <label className="field">
            <span className="field-label">{s.passwordAgain}</span>
            <input
              type="password"
              autoComplete="new-password"
              value={again}
              onChange={(e) => {
                setAgain(e.target.value);
              }}
            />
            {again !== "" && problem !== null && <span className="field-hint warn">{problem}</span>}
          </label>
          <div className="safe-actions">
            <button type="submit" className="button" disabled={problem !== null}>
              {s.passwordSave}
            </button>
            <button
              type="button"
              className="button button-quiet"
              onClick={() => {
                setEditing(false);
                setFirst("");
                setAgain("");
              }}
            >
              {strings.safe.cancel}
            </button>
          </div>
        </form>
      ) : (
        <div className="safe-actions">
          <button
            type="button"
            className="button button-quiet"
            onClick={() => {
              setNote(null);
              setEditing(true);
            }}
          >
            {sealed ? s.passwordChange : s.passwordSet}
          </button>
          {sealed && (
            <button
              type="button"
              className="button button-quiet"
              onClick={() => {
                save(null);
              }}
            >
              {s.passwordRemove}
            </button>
          )}
        </div>
      )}
      {note && (
        <p className="field-hint" role="status">
          {note}
        </p>
      )}
      {error && <ErrorNote error={error} />}
    </div>
  );
}

/** Settings > Storage: the weekly backup into a folder that the user picks. */
function AutoBackupView({ sealed }: { sealed: boolean }) {
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
      <p>{auto.folder === null ? s.autoOff(sealed) : s.autoOn(auto.folder, auto.last_ms, sealed)}</p>
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

export function Storage() {
  const s = strings.settings;
  const [state, setState] = useState<"idle" | "working" | Exported>("idle");
  const [importing, setImporting] = useState<"idle" | "working" | BackupInfo | Added>("idle");
  const [error, setError] = useState<UiError | null>(null);
  const [confirmReplace, setConfirmReplace] = useState(false);
  // Measures again after "Free up space".
  const [measured, setMeasured] = useState(0);
  const [lastBackup, setLastBackup] = useState<number | null | undefined>(undefined);
  const [sealed, setSealed] = useState(false);
  const [checked, setChecked] = useState(0);
  // The password step of an import: the zip stays picked on the Rust side.
  const [askPassword, setAskPassword] = useState(false);
  const [password, setPassword] = useState("");
  useEffect(() => {
    backupStatus()
      .then((b) => {
        setLastBackup(b.last_ms);
        setSealed(b.password);
      })
      .catch(() => {});
  }, [state, checked]);
  const fail = (e: unknown) => {
    setImporting("idle");
    const ui = toUiError(e);
    setAskPassword(ui.tip === "password");
    setError(ui);
  };
  const opening = (info: BackupInfo | null) => {
    setAskPassword(false);
    setPassword("");
    setImporting(info ?? "idle");
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
      <StatsView />
      <p>
        {s.backupIntro}
        {lastBackup !== undefined && ` ${s.lastBackup(lastBackup)}`}
      </p>
      <PasswordView
        sealed={sealed}
        onChange={() => {
          setChecked((n) => n + 1);
        }}
      />
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
              setAskPassword(false);
              openBackup().then(opening).catch(fail);
            }}
          >
            {importing === "working" ? s.importing : s.importBackup}
          </button>
        )}
      </div>
      {askPassword && opened === null && (
        <form
          className="password-form"
          onSubmit={(e) => {
            e.preventDefault();
            setImporting("working");
            setError(null);
            openBackup(password).then(opening).catch(fail);
          }}
        >
          <label className="field">
            <span className="field-label">{s.passwordOf}</span>
            <input
              type="password"
              autoComplete="off"
              value={password}
              onChange={(e) => {
                setPassword(e.target.value);
              }}
            />
          </label>
          <div className="safe-actions">
            <button type="submit" className="button" disabled={password === "" || importing === "working"}>
              {importing === "working" ? s.importing : s.passwordOpen}
            </button>
          </div>
        </form>
      )}
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
      <AutoBackupView sealed={sealed} />
    </section>
  );
}
