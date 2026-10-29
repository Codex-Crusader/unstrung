import { useRef, useState, type PointerEvent } from "react";
import { stagedUrl, stageUpload, toUiError, type AvatarChange, type Crop, type Staged } from "../api";
import { strings } from "../strings";
import { Avatar } from "./Avatar";

const BOX = 300;
const MIN_CROP = 32;
const MAX_BYTES = 15 * 1024 * 1024;

interface Owner {
  id: number;
  name: string;
  avatar_path: string | null;
}

/** Picks a picture file and a square crop. The core decodes and saves the picture. */
export function PictureField({
  kind,
  owner,
  value,
  onChange,
  initial,
  onStaged,
}: {
  kind: "character" | "persona";
  /** The saved record, to show its current picture. */
  owner: Owner | null;
  value: AvatarChange;
  onChange: (change: AvatarChange) => void;
  /** A picture that is already staged, for example from an imported card. */
  initial?: Staged | null;
  /** Reports the staged picture, so the creator can show the crop in its preview. */
  onStaged?: (staged: Staged | null) => void;
}) {
  const [staged, setStagedState] = useState<Staged | null>(initial ?? null);
  const setStaged = (next: Staged | null) => {
    setStagedState(next);
    onStaged?.(next);
  };
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const drag = useRef<{ startX: number; startY: number; crop: Crop } | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);

  const crop = value.kind === "staged" ? value.crop : null;
  const scale = staged ? Math.min(BOX / staged.width, BOX / staged.height) : 1;

  const setCrop = (next: Crop) => {
    if (!staged) return;
    const size = Math.round(Math.min(Math.max(next.size, MIN_CROP), staged.width, staged.height));
    const x = Math.round(Math.min(Math.max(next.x, 0), staged.width - size));
    const y = Math.round(Math.min(Math.max(next.y, 0), staged.height - size));
    onChange({ kind: "staged", token: staged.token, crop: { x, y, size } });
  };

  const choose = (file: File) => {
    setError(null);
    if (file.size > MAX_BYTES) {
      setError(strings.picture.tooLarge);
      return;
    }
    setBusy(true);
    file
      .arrayBuffer()
      .then((buffer) => stageUpload(new Uint8Array(buffer)))
      .then((s) => {
        setStaged(s);
        const size = Math.min(s.width, s.height);
        onChange({
          kind: "staged",
          token: s.token,
          crop: { x: Math.round((s.width - size) / 2), y: Math.round((s.height - size) / 2), size },
        });
      })
      .catch((e: unknown) => {
        setError(toUiError(e).message);
      })
      .finally(() => {
        setBusy(false);
      });
  };

  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    if (!crop) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    drag.current = { startX: e.clientX, startY: e.clientY, crop };
  };
  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    if (!d) return;
    setCrop({
      x: d.crop.x + (e.clientX - d.startX) / scale,
      y: d.crop.y + (e.clientY - d.startY) / scale,
      size: d.crop.size,
    });
  };
  const onPointerUp = () => {
    drag.current = null;
  };

  return (
    <div className="picture-field">
      {staged && crop ? (
        <div className="cropper">
          <div
            className="crop-stage"
            style={{ width: staged.width * scale, height: staged.height * scale }}
          >
            <img src={stagedUrl(staged.token)} alt={strings.picture.preview} draggable={false} />
            <div
              className="crop-box"
              role="presentation"
              style={{
                left: crop.x * scale,
                top: crop.y * scale,
                width: crop.size * scale,
                height: crop.size * scale,
              }}
              onPointerDown={onPointerDown}
              onPointerMove={onPointerMove}
              onPointerUp={onPointerUp}
              onPointerCancel={onPointerUp}
            />
          </div>
          <label className="crop-zoom">
            <span>{strings.picture.zoom}</span>
            <input
              type="range"
              min={MIN_CROP}
              max={Math.min(staged.width, staged.height)}
              value={crop.size}
              onChange={(e) => {
                const size = Number(e.target.value);
                const center = { x: crop.x + crop.size / 2, y: crop.y + crop.size / 2 };
                setCrop({ x: center.x - size / 2, y: center.y - size / 2, size });
              }}
            />
          </label>
          <p className="field-hint">{strings.picture.dragHint}</p>
        </div>
      ) : (
        <Avatar kind={kind} owner={value.kind === "remove" ? null : owner} size="large" />
      )}
      <div className="picture-actions">
        <button
          type="button"
          className="button button-quiet"
          disabled={busy}
          onClick={() => fileRef.current?.click()}
        >
          {busy ? strings.picture.reading : strings.picture.choose}
        </button>
        {staged && (
          <button
            type="button"
            className="link-button"
            onClick={() => {
              setStaged(null);
              onChange({ kind: "keep" });
            }}
          >
            {strings.picture.cancelNew}
          </button>
        )}
        {!staged && value.kind === "keep" && owner?.avatar_path && (
          <button
            type="button"
            className="link-button"
            onClick={() => {
              onChange({ kind: "remove" });
            }}
          >
            {strings.picture.remove}
          </button>
        )}
        {value.kind === "remove" && owner?.avatar_path && (
          <button
            type="button"
            className="link-button"
            onClick={() => {
              onChange({ kind: "keep" });
            }}
          >
            {strings.picture.undo}
          </button>
        )}
      </div>
      <input
        ref={fileRef}
        type="file"
        accept="image/png,image/jpeg,image/webp"
        hidden
        onChange={(e) => {
          const file = e.target.files?.[0];
          e.target.value = "";
          if (file) choose(file);
        }}
      />
      {error && (
        <p className="field-error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
