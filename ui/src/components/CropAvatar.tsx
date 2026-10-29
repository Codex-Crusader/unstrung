import { stagedUrl, type Crop, type Staged } from "../api";

const SIDE = 112;

/** A large avatar that shows a crop of a staged picture, before it is saved. */
export function CropAvatar({ staged, crop, label }: { staged: Staged; crop: Crop; label: string }) {
  const scale = SIDE / crop.size;
  return (
    <span className="avatar avatar-large crop-avatar">
      <img
        src={stagedUrl(staged.token)}
        alt={label}
        draggable={false}
        style={{
          width: staged.width * scale,
          height: staged.height * scale,
          left: -crop.x * scale,
          top: -crop.y * scale,
        }}
      />
    </span>
  );
}
