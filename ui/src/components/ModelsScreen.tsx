import { useEffect, useState } from "react";
import {
  downloadModel,
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

const GB = 1_000_000_000;
const CONTEXTS = [4096, 8192, 12288, 16384];

function gb(bytes: number): string {
  return (bytes / GB).toFixed(1);
}

/** Memory sizes: a card sold as "8 GB" has 8 GiB. */
function gib(bytes: number): string {
  return (bytes / 1024 ** 3).toFixed(1);
}

function DownloadLine({ download }: { download: Download }) {
  const s = strings.models;
  const text = {
    downloading: s.downloading(
      download.model,
      gb(download.completed),
      gb(download.total),
      download.bytes_per_second > 0 ? (download.bytes_per_second / 1_000_000).toFixed(1) : "",
    ),
    checking: s.checking(download.model),
    done: s.done(download.model),
    stopped: s.stopped(download.model),
    failed: s.failed(download.model),
  }[download.state];
  const active = download.state === "downloading" || download.state === "checking";
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

/** `engine` is the engine state and model. The list reloads when it changes,
 * because the engine finds the installed models only after it starts. */
export function ModelsScreen({ engine }: { engine: string }) {
  const s = strings.models;
  const [page, setPage] = useState<ModelsPage | null>(null);
  const [download, setDownload] = useState<Download | null>(null);
  const [error, setError] = useState<UiError | null>(null);
  const [confirm, setConfirm] = useState<string | null>(null);
  const [advanced, setAdvancedDraft] = useState<Advanced | null>(null);
  const [saved, setSaved] = useState(false);
  const [byName, setByName] = useState("");

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
        {error && (
          <p className="chat-error" role="alert">
            {s.loadFailed} {error.detail}
          </p>
        )}
      </section>
    );
  }

  const busy = download !== null && (download.state === "downloading" || download.state === "checking");
  const hw = page.hardware;
  const start = (model: string) => {
    setError(null);
    downloadModel(model).catch(fail);
  };

  const actions = (m: ModelView) => {
    if (busy && download.model === m.id) return null;
    if (!m.installed) {
      const resumable = download?.model === m.id && (download.state === "stopped" || download.state === "failed");
      return (
        <button
          type="button"
          className="button"
          disabled={busy}
          onClick={() => {
            start(m.id);
          }}
        >
          {resumable ? s.resume : s.download}
        </button>
      );
    }
    if (confirm === m.id) {
      return (
        <span className="confirm">
          {s.confirmRemove(m.name)}
          <button
            type="button"
            className="link-button danger"
            onClick={() => {
              setConfirm(null);
              removeModel(m.id).then(show).catch(fail);
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
      );
    }
    return (
      <>
        {!m.is_default && (
          <button
            type="button"
            className="button button-quiet"
            onClick={() => {
              setError(null);
              setDefaultModel(m.id).then(show).catch(fail);
            }}
          >
            {s.useDefault}
          </button>
        )}
        {!m.is_default && !page.shared_folder && (
          <button
            type="button"
            className="link-button danger"
            onClick={() => {
              setConfirm(m.id);
            }}
          >
            {s.remove}
          </button>
        )}
      </>
    );
  };

  return (
    <section className="page models">
      <h1>{s.title}</h1>
      <p className="muted">{s.intro}</p>
      {error && (
        <p className="chat-error" role="alert">
          {error.message} {error.detail}
        </p>
      )}

      <div className="pc-panel">
        <h2>{s.yourPc}</h2>
        <ul className="pc-facts">
          <li>{hw.gpu && hw.tier !== "basic" ? s.gpu(hw.gpu.name, gib(hw.gpu.vram_bytes)) : s.noGpu}</li>
          <li>{s.ram(gib(hw.ram_bytes))}</li>
          {hw.disk_free_bytes > 0 && <li>{s.disk(gb(hw.disk_free_bytes))}</li>}
          <li>
            {s.tier}: <strong>{s.tierName.get(hw.tier)}</strong>
          </li>
        </ul>
        {hw.below_minimum && <p className="field-hint warn">{s.belowMinimum}</p>}
        {page.hardware_changed && <p className="field-hint">{s.hardwareChanged}</p>}
        <button
          type="button"
          className="link-button"
          onClick={() => {
            recheckHardware().then(show).catch(fail);
          }}
        >
          {s.checkAgain}
        </button>
      </div>

      {download && <DownloadLine download={download} />}

      <ul className="model-list">
        {page.models.map((m) => (
          <li key={m.id} className={m.is_default ? "model-row model-default" : "model-row"}>
            <div className="model-info">
              <span className="model-name">
                {m.name}
                {m.recommended && <span className="badge">{s.recommended}</span>}
                {m.is_default && <span className="badge badge-strong">{s.isDefault}</span>}
                {!m.in_catalog && <span className="badge badge-quiet">{s.notTested}</span>}
                {!m.fits && <span className="badge badge-warn">{s.tooBig}</span>}
                {m.changed_upstream && <span className="badge badge-quiet">{s.changedUpstream}</span>}
              </span>
              {m.description && <span className="card-tagline">{m.description}</span>}
              <span className="model-facts">
                {s.size(gb(m.size_bytes))}
                {m.installed && ` · ${s.installed}`}
                {m.in_catalog && ` · ${s.styleFollowing.get(m.style_following) ?? ""}`}
                {m.can_think && ` · ${s.canThink}`}
                {m.license_name && (
                  <span title={m.license_url}>
                    {" "}
                    · {s.license}: {m.license_name}
                  </span>
                )}
              </span>
            </div>
            <div className="model-actions">{actions(m)}</div>
          </li>
        ))}
      </ul>
      {page.shared_folder && <p className="muted">{s.sharedFolder}</p>}

      <details className="more advanced">
        <summary>{s.advanced}</summary>
        <p className="muted">{s.advancedIntro}</p>
        {advanced && (
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
        )}
        <form
          className="by-name"
          onSubmit={(e) => {
            e.preventDefault();
            if (byName.trim()) start(byName.trim());
          }}
        >
          <label className="field">
            <span className="field-label">{s.byName}</span>
            <input
              value={byName}
              placeholder="llama3.1:8b"
              onChange={(e) => {
                setByName(e.target.value);
              }}
            />
            <span className="field-hint">{s.byNameHint}</span>
          </label>
          <button type="submit" className="button button-quiet" disabled={busy || !byName.trim()}>
            {s.add}
          </button>
        </form>
      </details>
    </section>
  );
}
