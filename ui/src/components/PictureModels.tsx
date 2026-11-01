import { useEffect, useState } from "react";
import {
  downloadPictureModel,
  onPictureDownload,
  picturesPage,
  removePictureModel,
  selectPictureModel,
  stopPictureDownload,
  toUiError,
  type PictureDownload,
  type PicturesPage,
  type UiError,
} from "../api";
import { strings } from "../strings";
import { Icon } from "./Icon";
import { ErrorNote } from "./ErrorNote";

function gb(bytes: number): string {
  return `${(bytes / 1_000_000_000).toFixed(1)} GB`;
}

/** True when pictures can be made now. */
export function picturesReady(page: PicturesPage): boolean {
  return page.engine_installed && page.models.some((m) => m.selected && m.installed);
}

/** The picture part of the Models screen: the model in use, and the others closed. */
export function PictureModels({ onSetUp }: { onSetUp: () => void }) {
  const s = strings.models;
  const p = strings.pictures;
  const [page, setPage] = useState<PicturesPage | null>(null);
  const [download, setDownload] = useState<PictureDownload | null>(null);
  const [error, setError] = useState<UiError | null>(null);

  const load = () => {
    picturesPage().then(
      (next) => {
        setPage(next);
        setDownload(next.download);
      },
      (e: unknown) => {
        setError(toUiError(e));
      },
    );
  };

  useEffect(() => {
    load();
    const off = onPictureDownload((d) => {
      setDownload(d);
      if (d && d.state !== "running") load();
    });
    return () => {
      void off.then((f) => {
        f();
      });
    };
  }, []);

  const act = (job: () => Promise<unknown>) => {
    setError(null);
    job().then(load, (e: unknown) => {
      setError(toUiError(e));
    });
  };

  if (!page) return error ? <ErrorNote error={error} /> : null;
  const inUse = page.models.find((m) => m.selected && m.installed);
  const busy = download?.state === "running";

  return (
    <div className="models-section">
      <h2>{s.picturesTitle}</h2>
      {error && <ErrorNote error={error} />}
      {picturesReady(page) && inUse ? (
        <p className="in-use-line">
          <span className="model-name">{inUse.name}</span>
          <span className="card-tagline">{inUse.description}</span>
        </p>
      ) : (
        <p className="in-use-line">
          <span className="muted">{s.picturesOff}</span>
          {!busy && (
            <button type="button" className="button" onClick={onSetUp}>
              <Icon name="picture" />
              {s.setUpPictures}
            </button>
          )}
        </p>
      )}
      {busy && (
        <div className="download-line" role="status">
          <span>{p.progress(gb(download.completed), gb(download.total))}</span>
          <progress max={download.total || 1} value={download.completed} />
          <button
            type="button"
            className="button button-quiet"
            onClick={() => {
              void stopPictureDownload();
            }}
          >
            {p.stop}
          </button>
        </div>
      )}
      {download?.state === "failed" && (
        <p className="field-hint warn">
          {p.failed} {download.error}
        </p>
      )}
      <details className="more">
        <summary>{s.morePictureModels}</summary>
        <ul className="model-list">
          {page.models.map((m) => (
            <li key={m.id} className={m.selected ? "model-row model-default" : "model-row"}>
              <div className="model-info">
                <span className="model-name">
                  {m.name}
                  {m.recommended && <span className="badge">{p.recommended}</span>}
                </span>
                <span className="card-tagline">{m.description}</span>
                <span className="muted" title={m.license_url}>
                  {gb(m.size_bytes)} · {p.license}: {m.license_name}
                </span>
              </div>
              <div className="model-actions">
                {!m.fits && !m.installed ? (
                  <span className="muted">{p.tooBig}</span>
                ) : !m.installed ? (
                  <button
                    type="button"
                    className="button button-quiet"
                    disabled={busy}
                    onClick={() => {
                      act(() => downloadPictureModel(m.id));
                    }}
                  >
                    <Icon name="download" />
                    {p.download(gb(m.missing_bytes))}
                  </button>
                ) : m.selected ? (
                  <span className="badge badge-strong">
                    <Icon name="check" />
                    {p.inUse}
                  </span>
                ) : (
                  <button
                    type="button"
                    className="button button-quiet"
                    onClick={() => {
                      act(() => selectPictureModel(m.id));
                    }}
                  >
                    {p.use}
                  </button>
                )}
                {m.installed && (
                  <button
                    type="button"
                    className="link-button danger"
                    disabled={busy}
                    onClick={() => {
                      act(() => removePictureModel(m.id));
                    }}
                  >
                    {p.remove}
                  </button>
                )}
              </div>
            </li>
          ))}
        </ul>
      </details>
    </div>
  );
}
