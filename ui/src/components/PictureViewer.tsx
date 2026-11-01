import { useEffect, useRef, useState } from "react";
import {
  deletePicture,
  exportPicture,
  onPainting,
  pictureUrl,
  repaintPicture,
  stagePicture,
  stopReply,
  toUiError,
  type GalleryPicture,
  type Painting,
  type Picture,
  type Staged,
  type UiError,
} from "../api";
import { strings } from "../strings";
import { Icon } from "./Icon";
import { ErrorNote } from "./ErrorNote";

/**
 * A picture that fades in once it has loaded, so it never pops in half drawn.
 * `develop` is the slow version for a new picture: it starts soft and blurred
 * and sharpens, like a photo that develops.
 */
export function FadeImage({
  picture,
  alt,
  develop = false,
  className,
  onShown,
}: {
  picture: { id: number; created_at: number };
  alt: string;
  develop?: boolean;
  className?: string;
  onShown?: () => void;
}) {
  const [loaded, setLoaded] = useState(false);
  const classes = ["fade-image", develop ? "develop" : "", loaded ? "loaded" : "", className ?? ""]
    .filter(Boolean)
    .join(" ");
  return (
    <img
      className={classes}
      src={pictureUrl(picture)}
      alt={alt}
      onLoad={() => {
        setLoaded(true);
      }}
      onTransitionEnd={(e) => {
        if (e.propertyName === "opacity") onShown?.();
      }}
    />
  );
}

/** The progress card that waits where the new picture will appear. */
export function PaintingCard({ painting }: { painting: Painting }) {
  const fraction = painting.phase === "painting" && painting.total > 0 ? painting.step / painting.total : null;
  return (
    <div className="painting-card" role="status">
      <Icon name="picture" />
      <span>{strings.chat.painting(painting.phase, painting.step, painting.total)}</span>
      {fraction !== null && <progress max={1} value={fraction} />}
      <button
        type="button"
        className="button button-quiet"
        onClick={() => {
          void stopReply();
        }}
      >
        {strings.pictures.stop}
      </button>
    </div>
  );
}

export type ViewerPicture = GalleryPicture;

/** Adds the character to a chat picture, for the viewer. */
export function withCharacter(p: Picture, characterId: number, name: string): ViewerPicture {
  return { ...p, character_id: characterId, character_name: name };
}

/**
 * One picture, large, over the page. Arrows (or the Left and Right keys) go
 * through `pictures`. A variation fades in on top of the picture it came from.
 */
export function PictureViewer({
  pictures,
  openId,
  onOpen,
  onClose,
  onMade,
  onDeleted,
  onOpenChat,
  onUseAsAvatar,
}: {
  pictures: ViewerPicture[];
  openId: number;
  onOpen: (id: number) => void;
  onClose: () => void;
  onMade: (picture: ViewerPicture) => void;
  onDeleted: (id: number) => void;
  /** Only in the gallery: the chat is already open elsewhere. */
  onOpenChat?: (picture: ViewerPicture) => void;
  onUseAsAvatar: (picture: ViewerPicture, staged: Staged) => void;
}) {
  const s = strings.pictures;
  const dialog = useRef<HTMLDialogElement>(null);
  const [painting, setPainting] = useState<Painting | null>(null);
  const [error, setError] = useState<UiError | null>(null);
  const [confirm, setConfirm] = useState(false);
  const [saved, setSaved] = useState(false);
  // The picture under the new one while a variation fades in.
  const [under, setUnder] = useState<ViewerPicture | null>(null);
  const [fresh, setFresh] = useState(false);

  const at = pictures.findIndex((p) => p.id === openId);
  const picture = pictures.find((p) => p.id === openId);
  const working = painting !== null;

  // Runs after each render: the dialog element comes back after a render
  // without a picture, and it must open as a modal again.
  useEffect(() => {
    const d = dialog.current;
    if (d && !d.open) d.showModal();
  });

  useEffect(() => {
    const off = onPainting((p) => {
      if (p) setPainting((now) => (now ? p : now));
    });
    return () => {
      void off.then((f) => {
        f();
      });
    };
  }, []);

  if (!picture) return null;

  const go = (step: number) => {
    const next = pictures.at(at + step);
    if (!next || working) return;
    setUnder(null);
    setFresh(false);
    setConfirm(false);
    setSaved(false);
    setError(null);
    onOpen(next.id);
  };

  const vary = () => {
    setError(null);
    setConfirm(false);
    setPainting({ chat_id: picture.chat_id, message_id: picture.message_id, phase: "scene", step: 0, total: 0 });
    repaintPicture(picture.id)
      .then((p) => {
        if (!p) return;
        setUnder(picture);
        setFresh(true);
        setSaved(false);
        onMade(withCharacter(p, picture.character_id, picture.character_name));
      })
      .catch((e: unknown) => {
        setError(toUiError(e));
      })
      .finally(() => {
        setPainting(null);
      });
  };

  return (
    <dialog
      ref={dialog}
      className="viewer"
      aria-label={picture.character_name}
      onCancel={(e) => {
        e.preventDefault();
        if (!working) onClose();
      }}
      onKeyDown={(e) => {
        if (e.key === "ArrowLeft") go(-1);
        if (e.key === "ArrowRight") go(1);
      }}
    >
      <div className="viewer-top">
        <span className="viewer-title">
          {picture.character_name}
          <span className="muted"> · {new Date(picture.created_at).toLocaleString()}</span>
        </span>
        <button type="button" className="icon-button" aria-label={s.close} disabled={working} onClick={onClose}>
          <Icon name="close" />
        </button>
      </div>

      <div className="viewer-stage">
        {pictures.length > 1 && (
          <button
            type="button"
            className="viewer-arrow viewer-prev"
            aria-label={s.previous}
            disabled={at <= 0 || working}
            onClick={() => {
              go(-1);
            }}
          >
            ‹
          </button>
        )}
        <div className={working ? "viewer-frame busy" : "viewer-frame"}>
          {under && <FadeImage key={`under-${String(under.id)}`} picture={under} alt="" className="viewer-under" />}
          <FadeImage
            key={picture.id}
            picture={picture}
            alt={strings.chat.picture}
            develop={fresh}
            onShown={() => {
              setUnder(null);
            }}
          />
          {painting && (
            <div className="viewer-progress">
              <PaintingCard painting={painting} />
            </div>
          )}
        </div>
        {pictures.length > 1 && (
          <button
            type="button"
            className="viewer-arrow viewer-next"
            aria-label={s.next}
            disabled={at >= pictures.length - 1 || working}
            onClick={() => {
              go(1);
            }}
          >
            ›
          </button>
        )}
      </div>

      {fresh && !working && <p className="viewer-note">{s.newVariation}</p>}
      {error && <ErrorNote error={error} />}

      <div className="viewer-actions">
        <button type="button" className="button" disabled={working} onClick={vary}>
          <Icon name="regenerate" />
          {s.variation}
        </button>
        {onOpenChat && (
          <button
            type="button"
            className="button button-quiet"
            disabled={working}
            onClick={() => {
              onOpenChat(picture);
            }}
          >
            <Icon name="chats" />
            {s.openChat}
          </button>
        )}
        <button
          type="button"
          className="button button-quiet"
          disabled={working}
          onClick={() => {
            setError(null);
            exportPicture(picture.id, picture.character_name).then(setSaved, (e: unknown) => {
              setError(toUiError(e));
            });
          }}
        >
          <Icon name={saved ? "check" : "download"} />
          {saved ? s.exported : s.export}
        </button>
        <button
          type="button"
          className="button button-quiet"
          disabled={working}
          onClick={() => {
            setError(null);
            stagePicture(picture.id).then(
              (staged) => {
                onUseAsAvatar(picture, staged);
              },
              (e: unknown) => {
                setError(toUiError(e));
              },
            );
          }}
        >
          <Icon name="personas" />
          {s.useAsAvatar}
        </button>
        <span className="spacer" />
        {confirm ? (
          <span className="viewer-confirm">
            {s.deleteConfirm}
            <button
              type="button"
              className="button danger-fill"
              onClick={() => {
                deletePicture(picture.id).then(
                  () => {
                    setConfirm(false);
                    onDeleted(picture.id);
                  },
                  (e: unknown) => {
                    setError(toUiError(e));
                  },
                );
              }}
            >
              {s.delete}
            </button>
            <button
              type="button"
              className="link-button"
              onClick={() => {
                setConfirm(false);
              }}
            >
              {s.keep}
            </button>
          </span>
        ) : (
          <button
            type="button"
            className="link-button danger"
            disabled={working}
            onClick={() => {
              setConfirm(true);
            }}
          >
            <Icon name="remove" />
            {s.delete}
          </button>
        )}
      </div>
    </dialog>
  );
}
