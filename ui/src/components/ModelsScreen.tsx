import { useEffect, useState, type ReactNode } from "react";
import {
  downloadModel,
  downloadRunning,
  getNotify,
  importModelFile,
  setNotify,
  modelsPage,
  onDownload,
  recheckHardware,
  removeModel,
  setAdvanced,
  setDefaultModel,
  stopDownload,
  toUiError,
  type Advanced,
  type Download,
  type ModelsPage,
  type ModelView,
  type UiError,
} from "../api";
import { strings } from "../strings";
import { Icon } from "./Icon";
import { ErrorNote } from "./ErrorNote";

const GB = 1_000_000_000;
const CONTEXTS = [4096, 8192, 12288, 16384];

function gb(bytes: number): string {
  return (bytes / GB).toFixed(1);
}

/** Memory sizes: a card sold as "8 GB" has 8 GiB. */
function gib(bytes: number): string {
  return (bytes / 1024 ** 3).toFixed(1);
}

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

/** `name` is the model name to show, or the tag for a model outside the catalog. */
function DownloadLine({ download, name, imported }: { download: Download; name: string; imported: boolean }) {
  const s = strings.models;
  const percent = String(Math.floor((100 * download.completed) / Math.max(1, download.total)));
  const text = {
    importing: s.importing(name, percent),
    downloading: s.downloading(
      name,
      gb(download.completed),
      gb(download.total),
      download.bytes_per_second > 0 ? (download.bytes_per_second / 1_000_000).toFixed(1) : "",
    ),
    checking: s.checking(name),
    done: imported ? s.imported(name) : s.done(name),
    stopped: imported ? s.importStopped(name) : s.stopped(name),
    failed: imported ? s.importFailed(name) : s.failed(name),
  }[download.state];
  const active = downloadRunning(download);
  return (
    <div className={download.state === "failed" ? "download-line download-failed" : "download-line"} role="status">
      <span>{text}</span>
      {download.state === "failed" && download.error && <span className="muted">{download.error}</span>}
      {active && (
        <>
          <progress max={download.total || 1} value={download.completed} />
          <button
            type="button"
            className="button button-quiet"
            onClick={() => {
              void stopDownload();
            }}
          >
            {s.stop}
          </button>
        </>
      )}
    </div>
  );
}

/** A number field with an "Automatic" box. */
function AutoNumber({
  label,
  value,
  step,
  min,
  max,
  fallback,
  onChange,
}: {
  label: string;
  value: number | null;
  step: number;
  min: number;
  max: number;
  fallback: number;
  onChange: (v: number | null) => void;
}) {
  return (
    <div className="field advanced-row">
      <span className="field-label">{label}</span>
      <label className="check">
        <input
          type="checkbox"
          checked={value === null}
          onChange={(e) => {
            onChange(e.target.checked ? null : fallback);
          }}
        />
        {strings.models.automatic}
      </label>
      {value !== null && (
        <input
          type="number"
          step={step}
          min={min}
          max={max}
          value={value}
          onChange={(e) => {
            const v = Number(e.target.value);
            if (Number.isFinite(v)) onChange(v);
          }}
        />
      )}
    </div>
  );
}

/** What the view can ask the screen to do. */
export interface ModelActions {
  download: (model: string) => void;
  use: (model: string) => void;
  remove: (model: string) => void;
  recheck: () => void;
  importFile: () => void;
}

const TIERS = ["basic", "standard", "high"] as const;

function bySize(a: ModelView, b: ModelView): number {
  return a.size_bytes - b.size_bytes;
}

/** The facts line under a model name. */
function Facts({ m }: { m: ModelView }) {
  const s = strings.models;
  // Only a warning when the model is too big. The group heading says what fits.
  const facts = [m.size_bytes > 0 ? s.size(gb(m.size_bytes)) : ""];
  if (m.in_catalog && !m.fits) facts.push(s.tooBig);
  if (m.in_catalog) facts.push(s.styleFollowing.get(m.style_following) ?? "");
  return (
    <span className="model-facts">
      {facts.filter(Boolean).join(" · ")}
      {m.license_name && (
        <span title={m.license_url}>
          {" "}
          · {s.license(m.license_name)}
        </span>
      )}
    </span>
  );
}

/** One model with its one main action. */
function ModelRow({
  m,
  page,
  download,
  confirm,
  setConfirm,
  act,
}: {
  m: ModelView;
  page: ModelsPage;
  download: Download | null;
  confirm: string | null;
  setConfirm: (id: string | null) => void;
  act: ModelActions;
}) {
  const s = strings.models;
  const busy = downloadRunning(download);
  const mine = download?.model === m.id;
  let main;
  if (busy && mine) {
    main = <span className="muted">{s.downloadingShort}</span>;
  } else if (!m.installed) {
    const resumable = mine && (download.state === "stopped" || download.state === "failed");
    main = (
      <button
        type="button"
        className="button"
        disabled={busy}
        onClick={() => {
          act.download(m.id);
        }}
      >
        <Icon name="download" />
        {resumable ? s.resume : s.download}
      </button>
    );
  } else if (m.is_default) {
    main = (
      <span className="badge badge-strong">
        <Icon name="check" />
        {s.inUse}
      </span>
    );
  } else {
    main = (
      <button
        type="button"
        className="button button-quiet"
        onClick={() => {
          act.use(m.id);
        }}
      >
        <Icon name="check" />
        {s.useForNew}
      </button>
    );
  }
  const removable = m.installed && !m.is_default && !page.shared_folder;
  return (
    <li className={m.is_default ? "model-row model-default" : "model-row"}>
      <div className="model-info">
        <span className="model-name">
          {m.name}
          {m.recommended && <span className="badge">{s.recommended}</span>}
          {m.installed && !m.is_default && <span className="badge badge-quiet">{s.installed}</span>}
          {m.changed_upstream && <span className="badge badge-quiet">{s.changedUpstream}</span>}
        </span>
        {m.in_catalog ? (
          m.description && <span className="card-tagline">{m.description}</span>
        ) : (
          <span className="card-tagline">{s.ownNotTested}</span>
        )}
        <Facts m={m} />
      </div>
      <div className="model-actions">
        {confirm === m.id ? (
          <span className="confirm">
            {s.confirmRemove(m.name)}
            <button
              type="button"
              className="link-button danger"
              onClick={() => {
                setConfirm(null);
                act.remove(m.id);
              }}
            >
              {s.yesRemove}
            </button>
            <button
              type="button"
              className="link-button"
              onClick={() => {
                setConfirm(null);
              }}
            >
              {s.cancel}
            </button>
          </span>
        ) : (
          <>
            {main}
            {removable && (
              <button
                type="button"
                className="link-button danger"
                onClick={() => {
                  setConfirm(m.id);
                }}
              >
                <Icon name="remove" />
                {s.remove}
              </button>
            )}
          </>
        )}
      </div>
    </li>
  );
}

/** The Models screen without state, so it can render from fixed data. */
export function ModelsView({
  page,
  download,
  imported,
  act,
  notice,
  ownForm,
  advancedForm,
}: {
  notice: ReactNode;
  page: ModelsPage;
  download: Download | null;
  /** The model name of the last import, so its line says "import". */
  imported: string | null;
  act: ModelActions;
  ownForm: ReactNode;
  advancedForm: ReactNode;
}) {
  const s = strings.models;
  const [confirm, setConfirm] = useState<string | null>(null);
  const hw = page.hardware;
  const catalog = page.models.filter((m) => m.in_catalog);
  const own = page.models.filter((m) => !m.in_catalog).sort(bySize);
  const installed = page.models.filter((m) => m.installed);
  const pick = catalog.find((m) => m.recommended);
  const busy = downloadRunning(download);
  const row = (m: ModelView) => (
    <ModelRow key={m.id} m={m} page={page} download={download} confirm={confirm} setConfirm={setConfirm} act={act} />
  );

  return (
    <section className="page models">
      <h1>{s.title}</h1>
      <p className="muted">{s.intro}</p>
      {notice}

      <div className="pc-panel">
        <p className="pc-line">
          <strong>{s.canRun.get(hw.tier)}</strong>
          <button type="button" className="link-button" onClick={act.recheck}>
            <Icon name="refresh" />
            {s.checkAgain}
          </button>
        </p>
        <p className="pc-facts" aria-label={s.yourPc}>
          <span className="pc-fact">
            <Icon name="gpu" />
            {hw.gpu && hw.tier !== "basic" ? s.gpu(hw.gpu.name, gib(hw.gpu.vram_bytes)) : s.noGpu}
          </span>
          <span className="pc-fact">
            <Icon name="ram" />
            {s.ram(gib(hw.ram_bytes))}
          </span>
          {hw.disk_free_bytes > 0 && (
            <span className="pc-fact">
              <Icon name="disk" />
              {s.disk(gb(hw.disk_free_bytes))}
            </span>
          )}
        </p>
        {hw.below_minimum && <p className="field-hint warn">{s.belowMinimum}</p>}
        {page.hardware_changed && <p className="field-hint">{s.hardwareChanged}</p>}
      </div>

      {download && (
        <DownloadLine
          download={download}
          name={page.models.find((m) => m.id === download.model)?.name ?? download.model}
          imported={download.model === imported}
        />
      )}

      {/* When new chats already use it, the list marks it. */}
      {pick && !(pick.installed && pick.is_default) && (
        <div className="recommended-card">
          <h2>{s.recommendedTitle}</h2>
          <p className="model-name">{pick.name}</p>
          <p className="card-tagline">{pick.description}</p>
          <Facts m={pick} />
          <div className="model-actions">
            {pick.installed ? (
              <button
                type="button"
                className="button"
                onClick={() => {
                  act.use(pick.id);
                }}
              >
                <Icon name="check" />
                {s.useForNew}
              </button>
            ) : (
              <button
                type="button"
                className="button button-big"
                disabled={busy}
                onClick={() => {
                  act.download(pick.id);
                }}
              >
                <Icon name="download" />
                {busy && download?.model === pick.id ? s.downloadingShort : `${s.download} (${gb(pick.size_bytes)} GB)`}
              </button>
            )}
          </div>
        </div>
      )}

      <div className="models-section">
        <h2>{s.inUseTitle}</h2>
        {installed.length === 0 ? (
          <p className="muted">{s.noModelYet}</p>
        ) : (
          <label className="field in-use">
            <select
              value={page.default_model}
              onChange={(e) => {
                act.use(e.target.value);
              }}
            >
              {installed.every((m) => m.id !== page.default_model) && (
                <option value={page.default_model}>{page.default_model}</option>
              )}
              {installed.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.name}
                </option>
              ))}
            </select>
            <span className="field-hint">{s.inUseHint}</span>
          </label>
        )}
        <NotifyToggle />
      </div>

      <div className="models-section">
        <h2>{s.allTitle}</h2>
        {TIERS.map((tier) => {
          const group = catalog.filter((m) => (m.min_tier ?? "basic") === tier).sort(bySize);
          if (group.length === 0) return null;
          return (
            <div key={tier} className="model-group">
              <h3>{s.group.get(tier)}</h3>
              <ul className="model-list">{group.map(row)}</ul>
            </div>
          );
        })}
        {page.shared_folder && <p className="muted">{s.sharedFolder}</p>}
      </div>

      <div className="models-section">
        <h2>{s.ownTitle}</h2>
        <p className="muted">{s.ownIntro}</p>
        {own.length > 0 ? <ul className="model-list">{own.map(row)}</ul> : <p className="muted">{s.ownNone}</p>}
        {ownForm}
        <div className="import-file">
          <button
            type="button"
            className="button button-quiet"
            disabled={busy}
            onClick={act.importFile}
          >
            <Icon name="import" />
            {s.importFile}
          </button>
          <span className="field-hint">{s.importHint}</span>
          <p className="welcome-warning">
            <Icon name="warning" />
            {s.importWarning}
          </p>
        </div>
      </div>

      <details className="more advanced">
        <summary>{s.advanced}</summary>
        <p className="muted">{s.advancedIntro}</p>
        {advancedForm}
      </details>
    </section>
  );
}

/** `engine` is the engine state and model. The list reloads when it changes,
 * because the engine finds the installed models only after it starts. */
export function ModelsScreen({ engine }: { engine: string }) {
  const s = strings.models;
  const [page, setPage] = useState<ModelsPage | null>(null);
  const [download, setDownload] = useState<Download | null>(null);
  const [error, setError] = useState<UiError | null>(null);
  const [ownError, setOwnError] = useState<UiError | null>(null);
  const [advanced, setAdvancedDraft] = useState<Advanced | null>(null);
  const [saved, setSaved] = useState(false);
  const [byName, setByName] = useState("");
  const [imported, setImported] = useState<string | null>(null);

  const show = (p: ModelsPage) => {
    setPage(p);
    setDownload(p.download);
    setAdvancedDraft(p.advanced);
  };
  const fail = (e: unknown) => {
    setError(toUiError(e));
  };

  useEffect(() => {
    let active = true;
    modelsPage()
      .then((p) => {
        if (active) show(p);
      })
      .catch((e: unknown) => {
        if (active) fail(e);
      });
    const unlisten = onDownload((d) => {
      if (!active) return;
      setDownload(d);
      // A finished download changes the list.
      if (d && d.state !== "downloading" && d.state !== "checking") {
        modelsPage()
          .then((p) => {
            if (active) setPage(p);
          })
          .catch(() => {
            // The list stays as it was. The download line shows the result.
          });
      }
    });
    return () => {
      active = false;
      void unlisten.then((stop) => {
        stop();
      });
    };
  }, []);

  useEffect(() => {
    let active = true;
    modelsPage()
      .then((p) => {
        if (active) setPage(p);
      })
      .catch(() => {
        // The list stays as it was.
      });
    return () => {
      active = false;
    };
  }, [engine]);

  if (!page) {
    return (
      <section className="page models">
        <h1>{s.title}</h1>
        {error && <ErrorNote error={error} lead={s.loadFailed} />}
      </section>
    );
  }

  const busy = downloadRunning(download);
  const act: ModelActions = {
    download: (model) => {
      setError(null);
      downloadModel(model).catch(fail);
    },
    use: (model) => {
      setError(null);
      setDefaultModel(model).then(show).catch(fail);
    },
    remove: (model) => {
      setError(null);
      removeModel(model).then(show).catch(fail);
    },
    recheck: () => {
      recheckHardware().then(show).catch(fail);
    },
    importFile: () => {
      setError(null);
      importModelFile()
        .then((name) => {
          if (name !== null) setImported(name);
        })
        .catch(fail);
    },
  };

  const ownForm = (
    <form
      className="by-name"
      onSubmit={(e) => {
        e.preventDefault();
        const name = byName.trim();
        if (!name) return;
        setOwnError(null);
        downloadModel(name)
          .then(() => {
            setByName("");
          })
          .catch((err: unknown) => {
            setOwnError(toUiError(err));
          });
      }}
    >
      <label className="field">
        <span className="field-label">{s.byName}</span>
        <input
          type="text"
          value={byName}
          placeholder="llama3.1:8b"
          spellCheck={false}
          onChange={(e) => {
            setByName(e.target.value);
          }}
        />
        <span className="field-hint">{s.byNameHint}</span>
        {ownError && <ErrorNote error={ownError} />}
      </label>
      <button type="submit" className="button" disabled={busy || !byName.trim()}>
        {s.add}
      </button>
    </form>
  );

  const advancedForm = advanced && (
    <form
      className="advanced-form"
      onSubmit={(e) => {
        e.preventDefault();
        setError(null);
        setSaved(false);
        setAdvanced(advanced)
          .then((p) => {
            show(p);
            setSaved(true);
          })
          .catch(fail);
      }}
    >
      <label className="field advanced-row">
        <span className="field-label">{s.context}</span>
        <select
          value={advanced.context ?? ""}
          onChange={(e) => {
            setAdvancedDraft({ ...advanced, context: e.target.value ? Number(e.target.value) : null });
          }}
        >
          <option value="">{s.automatic}</option>
          {CONTEXTS.map((c) => (
            <option key={c} value={c}>
              {s.tokens(c)}
            </option>
          ))}
        </select>
        <span className="field-hint">{s.contextHint}</span>
      </label>
      <label className="field advanced-row">
        <span className="field-label">{s.think}</span>
        <select
          value={advanced.think === null ? "" : advanced.think ? "on" : "off"}
          onChange={(e) => {
            const v = e.target.value;
            setAdvancedDraft({ ...advanced, think: v === "" ? null : v === "on" });
          }}
        >
          <option value="">{s.automatic}</option>
          <option value="on">{s.on}</option>
          <option value="off">{s.off}</option>
        </select>
        <span className="field-hint">{s.thinkHint}</span>
      </label>
      <AutoNumber
        label={s.temperature}
        value={advanced.temperature}
        step={0.05}
        min={0}
        max={2}
        fallback={0.8}
        onChange={(v) => {
          setAdvancedDraft({ ...advanced, temperature: v });
        }}
      />
      <AutoNumber
        label={s.minP}
        value={advanced.min_p}
        step={0.01}
        min={0}
        max={0.5}
        fallback={0.05}
        onChange={(v) => {
          setAdvancedDraft({ ...advanced, min_p: v });
        }}
      />
      <AutoNumber
        label={s.repeatPenalty}
        value={advanced.repeat_penalty}
        step={0.01}
        min={1}
        max={1.5}
        fallback={1.08}
        onChange={(v) => {
          setAdvancedDraft({ ...advanced, repeat_penalty: v });
        }}
      />
      <div className="form-actions">
        {saved && <span className="muted">{s.saved}</span>}
        <span className="spacer" />
        <button type="submit" className="button">
          {s.save}
        </button>
      </div>
    </form>
  );

  const notice = error && <ErrorNote error={error} />;
  return (
    <ModelsView
      page={page}
      download={download}
      imported={imported}
      act={act}
      notice={notice}
      ownForm={ownForm}
      advancedForm={advancedForm}
    />
  );
}
