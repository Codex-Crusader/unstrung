import { useEffect, useState } from "react";
import {
  downloadPictureModel,
  gallery,
  onPictureDownload,
  picturesPage,
  removePictureModel,
  selectPictureModel,
  stopPictureDownload,
  toUiError,
  type GalleryPicture,
  type PictureDownload,
  type PictureModel,
  type PicturesPage,
  type Staged,
  type UiError,
} from "../api";
import { strings } from "../strings";
import { Icon } from "./Icon";
import { ErrorNote } from "./ErrorNote";
import { FadeImage, PictureViewer } from "./PictureViewer";

function gb(bytes: number): string {
  return `${(bytes / 1_000_000_000).toFixed(1)} GB`;
}

/** True when pictures can be made now. */
function isReady(page: PicturesPage): boolean {
  return page.engine_installed && page.models.some((m) => m.selected && m.installed);
}

/** The model that "Turn on pictures" sets up. */
function firstChoice(page: PicturesPage): PictureModel | undefined {
  return page.models.find((m) => m.recommended && m.fits) ?? page.models.find((m) => m.fits);
}

/** The one big step for a user who has no pictures yet. */
function SetupCard({
  page,
  download,
  act,
}: {
  page: PicturesPage;
  download: PictureDownload | null;
  act: (job: () => Promise<unknown>) => void;
}) {
  const s = strings.pictures;
  const model = firstChoice(page);
  if (!model) return <p className="muted">{s.noModelFits}</p>;
  const running = download?.state === "running";
  const size = model.missing_bytes + (page.engine_installed ? 0 : page.engine_download_bytes);
  const start = () => {
    act(() => (model.installed ? selectPictureModel(model.id) : downloadPictureModel(model.id)));
  };

  return (
    <div className="setup-card">
      <Icon name="picture" className="setup-icon" />
      <h2>{s.setupTitle}</h2>
      <p>{s.setupText}</p>
      {page.cpu_only && <p className="field-hint warn">{s.cpuOnly}</p>}
      {running ? (
        <div className="setup-progress" role="status">
          <span>
            {download.part === "engine" ? s.stepEngine : page.engine_installed ? s.stepModelOnly : s.stepModel}
          </span>
          <progress max={download.total || 1} value={download.completed} />
          <span className="muted">
            {s.progress(gb(download.completed), gb(download.total))}
            {download.bytes_per_second > 0 &&
              ` · ${s.timeLeft(Math.max(1, Math.ceil((download.total - download.completed) / download.bytes_per_second / 60)))}`}
          </span>
          <button
            type="button"
            className="button button-quiet"
            onClick={() => {
              void stopPictureDownload();
            }}
          >
            {s.stop}
          </button>
        </div>
      ) : (
        <>
          {download?.state === "failed" && (
            <p className="field-hint warn">
              {s.failed} {download.error}
            </p>
          )}
          {download?.state === "stopped" && <p className="muted">{s.stopped}</p>}
          <button type="button" className="button button-big" onClick={start}>
            {!model.installed && download?.state === "stopped" ? s.continueDownload : s.turnOn}
          </button>
          {!model.installed && <p className="muted">{s.downloadOnce(gb(size))}</p>}
        </>
      )}
    </div>
  );
}

/** The picture model list, for people who want another model. Closed at first. */
function Settings({
  page,
  busy,
  act,
}: {
  page: PicturesPage;
  busy: boolean;
  act: (job: () => Promise<unknown>) => void;
}) {
  const s = strings.pictures;
  return (
    <details className="picture-settings">
      <summary>{s.settings}</summary>
      <p className="muted">{s.settingsText}</p>
      <ul className="model-list">
        {page.models.map((m) => (
          <li key={m.id} className={m.selected ? "model-row model-default" : "model-row"}>
            <div className="model-info">
              <span className="model-name">
                {m.name}
                {m.recommended && <span className="badge">{s.recommended}</span>}
              </span>
              <span className="card-tagline">{m.description}</span>
              <span className="muted" title={m.license_url}>
                {gb(m.size_bytes)} · {s.license}: {m.license_name}
              </span>
            </div>
            <div className="model-actions">
              {!m.fits && !m.installed ? (
                <span className="muted">{s.tooBig}</span>
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
        ))}
      </ul>
    </details>
  );
}

export function PicturesScreen({
  onOpenChat,
  onUseAsAvatar,
}: {
  onOpenChat: (characterId: number, chatId: number) => void;
  onUseAsAvatar: (picture: GalleryPicture, staged: Staged) => void;
}) {
  const s = strings.pictures;
  const [page, setPage] = useState<PicturesPage | null>(null);
  const [download, setDownload] = useState<PictureDownload | null>(null);
  const [pictures, setPictures] = useState<GalleryPicture[]>([]);
  const [who, setWho] = useState<number | null>(null);
  const [open, setOpen] = useState<number | null>(null);
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

  const people = [...new Map(pictures.map((p) => [p.character_id, p.character_name])).entries()];
  const shown = who === null ? pictures : pictures.filter((p) => p.character_id === who);
  const ready = page !== null && isReady(page);

  return (
    <section className="page pictures">
      <h1>{s.title}</h1>
      {error && <ErrorNote error={error} />}

      {page && !ready && <SetupCard page={page} download={download} act={act} />}

      {(ready || pictures.length > 0) && (
        <>
          {people.length > 1 && (
            <div className="chips" role="group" aria-label={s.show}>
              <button
                type="button"
                className={who === null ? "chip chip-on" : "chip"}
                aria-pressed={who === null}
                onClick={() => {
                  setWho(null);
                }}
              >
                {s.everyone}
              </button>
              {people.map(([id, name]) => (
                <button
                  key={id}
                  type="button"
                  className={who === id ? "chip chip-on" : "chip"}
                  aria-pressed={who === id}
                  onClick={() => {
                    setWho(id);
                  }}
                >
                  {name}
                </button>
              ))}
            </div>
          )}
          {shown.length === 0 ? (
            <div className="empty-pictures">
              <Icon name="picture" className="setup-icon" />
              <p>{s.emptyTitle}</p>
              <p className="muted">{s.emptyText}</p>
            </div>
          ) : (
            <ul className="picture-grid">
              {shown.map((p) => (
                <li key={p.id}>
                  <button
                    type="button"
                    className="picture-thumb"
                    aria-label={`${strings.chat.openPicture}: ${p.character_name}`}
                    onClick={() => {
                      setOpen(p.id);
                    }}
                  >
                    <FadeImage picture={p} alt="" />
                    <span className="picture-name">{p.character_name}</span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </>
      )}

      {page && (ready || download?.state === "running") && (
        <Settings page={page} busy={download?.state === "running"} act={act} />
      )}

      {open !== null && (
        <PictureViewer
          pictures={shown}
          openId={open}
          onOpen={setOpen}
          onClose={() => {
            setOpen(null);
          }}
          onMade={(p) => {
            setPictures((all) => [p, ...all.filter((x) => x.id !== p.id)]);
            setOpen(p.id);
          }}
          onDeleted={(id) => {
            const at = shown.findIndex((p) => p.id === id);
            const next = shown[at + 1] ?? shown[at - 1];
            setPictures((all) => all.filter((p) => p.id !== id));
            setOpen(next ? next.id : null);
          }}
          onOpenChat={(p) => {
            onOpenChat(p.character_id, p.chat_id);
          }}
          onUseAsAvatar={onUseAsAvatar}
        />
      )}
    </section>
  );
}
