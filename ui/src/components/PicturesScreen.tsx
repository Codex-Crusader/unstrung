import { useEffect, useState } from "react";
import {
  deletePicture,
  downloadPictureModel,
  gallery,
  onPictureDownload,
  picturesPage,
  pictureUrl,
  removePictureModel,
  repaintPicture,
  stopReply,
  selectPictureModel,
  stopPictureDownload,
  toUiError,
  type GalleryPicture,
  type PictureDownload,
  type PictureModel,
  type PicturesPage,
  type UiError,
} from "../api";
import { strings } from "../strings";
import { Icon } from "./Icon";
import { ErrorNote } from "./ErrorNote";

function gb(bytes: number): string {
  return `${(bytes / 1_000_000_000).toFixed(1)} GB`;
}

function mb(bytes: number): string {
  return `${String(Math.ceil(bytes / 1_000_000))} MB`;
}

function DownloadLine({ download }: { download: PictureDownload }) {
  const s = strings.pictures;
  if (download.state === "done") return null;
  return (
    <div className={download.state === "failed" ? "download-line download-failed" : "download-line"} role="status">
      {download.state === "running" && (
        <>
          <span>{s.downloading(download.part, gb(download.completed), gb(download.total))}</span>
          <progress max={download.total || 1} value={download.completed} />
          <button
            type="button"
            className="button button-quiet"
            onClick={() => {
              void stopPictureDownload();
            }}
          >
            {s.stop}
          </button>
        </>
      )}
      {download.state === "stopped" && <span>{s.stopped}</span>}
      {download.state === "failed" && (
        <>
          <span>{s.failed}</span>
          {download.error && <span className="muted">{download.error}</span>}
        </>
      )}
    </div>
  );
}

function ModelRow({
  m,
  busy,
  act,
}: {
  m: PictureModel;
  busy: boolean;
  act: (job: () => Promise<unknown>) => void;
}) {
  const s = strings.pictures;
  return (
    <li className={m.selected ? "model-row model-default" : "model-row"}>
      <div className="model-info">
        <span className="model-name">
          {m.name}
          {m.recommended && <span className="badge">{s.recommended}</span>}
          {!m.fits && <span className="badge badge-quiet">{s.tooBig}</span>}
          {m.mature_ok && <span className="badge badge-quiet">{s.adultOk}</span>}
        </span>
        <span className="card-tagline">{m.description}</span>
        <span className="muted">
          {gb(m.size_bytes)} ·{" "}
          <span title={m.license_url}>
            {s.license}: {m.license_name}
          </span>
          {!m.tested && <> · {s.untested}</>}
        </span>
      </div>
      <div className="model-actions">
        {!m.installed ? (
          <button
            type="button"
            className="button"
            disabled={busy}
            onClick={() => {
              act(() => downloadPictureModel(m.id));
            }}
          >
            <Icon name="download" />
            {s.download(gb(m.missing_bytes))}
          </button>
        ) : m.selected ? (
          <span className="badge badge-strong">
            <Icon name="check" />
            {s.inUse}
          </span>
        ) : (
          <button
            type="button"
            className="button button-quiet"
            onClick={() => {
              act(() => selectPictureModel(m.id));
            }}
          >
            <Icon name="check" />
            {s.use}
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
            {s.remove}
          </button>
        )}
      </div>
    </li>
  );
}

/** One picture, large, with its actions. */
function PictureView({
  picture,
  onClose,
  onOpenChat,
  onDeleted,
  onMade,
}: {
  picture: GalleryPicture;
  onClose: () => void;
  onOpenChat: (characterId: number, chatId: number) => void;
  onDeleted: () => void;
  onMade: () => void;
}) {
  const s = strings.pictures;
  const [confirm, setConfirm] = useState(false);
  const [error, setError] = useState<UiError | null>(null);
  const [working, setWorking] = useState(false);
  const again = (sameSeed: boolean) => {
    setError(null);
    setWorking(true);
    repaintPicture(picture.id, sameSeed)
      .then((p) => {
        if (p) onMade();
      })
      .catch((e: unknown) => {
        setError(toUiError(e));
      })
      .finally(() => {
        setWorking(false);
      });
  };
  return (
    <div className="picture-view" role="dialog" aria-label={picture.character_name}>
      <img src={pictureUrl(picture)} alt={strings.chat.picture} />
      <p className="muted">
        {picture.character_name} · {new Date(picture.created_at).toLocaleString()}
      </p>
      {error && <ErrorNote error={error} />}
      {working && (
        <p className="painting-line" role="status">
          <Icon name="picture" />
          {s.repainting}
          <button
            type="button"
            className="button button-quiet"
            onClick={() => {
              void stopReply();
            }}
          >
            {s.stop}
          </button>
        </p>
      )}
      <div className="form-actions">
        <button
          type="button"
          className="button"
          onClick={() => {
            onOpenChat(picture.character_id, picture.chat_id);
          }}
        >
          {s.openChat}
        </button>
        <button
          type="button"
          className="button button-quiet"
          disabled={working}
          onClick={() => {
            again(true);
          }}
        >
          <Icon name="regenerate" />
          {s.again}
        </button>
        <button
          type="button"
          className="button button-quiet"
          disabled={working}
          onClick={() => {
            again(false);
          }}
        >
          <Icon name="picture" />
          {s.variation}
        </button>
        {confirm ? (
          <>
            <span>{s.deleteConfirm}</span>
            <button
              type="button"
              className="button danger"
              onClick={() => {
                deletePicture(picture.id).then(onDeleted, (e: unknown) => {
                  setError(toUiError(e));
                });
              }}
            >
              {s.delete}
            </button>
          </>
        ) : (
          <button
            type="button"
            className="link-button danger"
            onClick={() => {
              setConfirm(true);
            }}
          >
            {s.delete}
          </button>
        )}
        <span className="spacer" />
        <button type="button" className="button button-quiet" onClick={onClose}>
          <Icon name="close" />
          {s.close}
        </button>
      </div>
    </div>
  );
}

export function PicturesScreen({ onOpenChat }: { onOpenChat: (characterId: number, chatId: number) => void }) {
  const s = strings.pictures;
  const [page, setPage] = useState<PicturesPage | null>(null);
  const [download, setDownload] = useState<PictureDownload | null>(null);
  const [pictures, setPictures] = useState<GalleryPicture[]>([]);
  const [who, setWho] = useState<number | null>(null);
  const [open, setOpen] = useState<GalleryPicture | null>(null);
  const [error, setError] = useState<UiError | null>(null);

  const load = () => {
    picturesPage().then(
      (p) => {
        setPage(p);
        setDownload(p.download);
      },
      (e: unknown) => {
        setError(toUiError(e));
      },
    );
    gallery().then(setPictures, (e: unknown) => {
      setError(toUiError(e));
    });
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

  const busy = download?.state === "running";
  const people = [...new Map(pictures.map((p) => [p.character_id, p.character_name])).entries()];
  const shown = who === null ? pictures : pictures.filter((p) => p.character_id === who);

  return (
    <section className="page models">
      <h1>{s.title}</h1>
      <p className="muted">{s.intro}</p>
      {page?.cpu_only && <p className="field-hint warn">{s.cpuOnly}</p>}
      {error && <ErrorNote error={error} />}

      <div className="models-section">
        <h2>{s.models}</h2>
        {page && !page.engine_installed && <p className="muted">{s.engine(mb(page.engine_download_bytes))}</p>}
        {download && <DownloadLine download={download} />}
        <ul className="model-list">
          {page?.models.map((m) => (
            <ModelRow key={m.id} m={m} busy={busy} act={act} />
          ))}
        </ul>
      </div>

      <div className="models-section">
        <h2>{s.gallery}</h2>
        {people.length > 1 && (
          <select
            aria-label={s.gallery}
            value={who ?? ""}
            onChange={(e) => {
              setWho(e.target.value === "" ? null : Number(e.target.value));
            }}
          >
            <option value="">{s.all}</option>
            {people.map(([id, name]) => (
              <option key={id} value={id}>
                {name}
              </option>
            ))}
          </select>
        )}
        {open ? (
          <PictureView
            picture={open}
            onClose={() => {
              setOpen(null);
            }}
            onOpenChat={onOpenChat}
            onDeleted={() => {
              setOpen(null);
              load();
            }}
            onMade={load}
          />
        ) : shown.length === 0 ? (
          <p className="muted">{s.empty}</p>
        ) : (
          <ul className="picture-grid">
            {shown.map((p) => (
              <li key={p.id}>
                <button
                  type="button"
                  className="picture-thumb"
                  aria-label={`${strings.chat.openPicture}: ${p.character_name}`}
                  onClick={() => {
                    setOpen(p);
                  }}
                >
                  <img src={pictureUrl(p)} alt="" loading="lazy" />
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </section>
  );
}
