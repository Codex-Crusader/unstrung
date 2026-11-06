import { strings } from "../strings";
import figure from "../assets/figure.png";

/**
 * Three strings that hang from a bar and sway, while data loads. It fades
 * in after a short delay, so a fast load shows nothing at all.
 */
export function Loading({ label = strings.loading.label }: { label?: string }) {
  return (
    <div className="loading" role="status">
      <svg className="loading-strings" viewBox="0 0 64 56" aria-hidden="true">
        <line className="loading-bar" x1="8" y1="4" x2="56" y2="4" />
        {[18, 32, 46].map((x, i) => (
          <g key={x} className={`loading-string loading-string-${String(i + 1)}`} style={{ transformOrigin: `${String(x)}px 4px` }}>
            <line x1={x} y1="4" x2={x} y2={30 + i * 6} />
            <circle cx={x} cy={33 + i * 6} r="3.5" />
          </g>
        ))}
      </svg>
      <span className="loading-label">{label}</span>
    </div>
  );
}

/** The whole window while the app starts: the same picture as the splash in index.html. */
export function Splash() {
  return (
    <div className="splash-screen">
      <img src={figure} alt="" />
      <span className="splash-name">Unstrung</span>
      <Loading />
    </div>
  );
}
