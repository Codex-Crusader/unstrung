import type { ReactNode } from "react";

/**
 * The app's icons, drawn like the logo figure: thick rounded strokes in the
 * text color, and at most one brass pin (the figure's joints) in the accent
 * color. 24 px grid. Each one must still read at 16 px.
 */
const PIN = (cx: number, cy: number) => <circle className="icon-pin" cx={cx} cy={cy} r={1.9} />;

const ICONS = {
  home: (
    <>
      <path d="M4 11 12 4.5 20 11v8a1.5 1.5 0 0 1-1.5 1.5H15v-5.5H9v5.5H5.5A1.5 1.5 0 0 1 4 19z" />
      {PIN(12, 10.5)}
    </>
  ),
  personas: (
    <>
      <ellipse cx={12} cy={8} rx={3.3} ry={3.8} />
      <path d="M5 20.5c.6-3.9 3.4-6.5 7-6.5s6.4 2.6 7 6.5" />
      {PIN(12, 16.8)}
    </>
  ),
  models: (
    <>
      <rect x={6} y={6} width={12} height={12} rx={2.5} />
      <path d="M9.5 3v3M14.5 3v3M9.5 18v3M14.5 18v3M3 9.5h3M3 14.5h3M18 9.5h3M18 14.5h3" />
      {PIN(12, 12)}
    </>
  ),
  back: <path d="M14.5 5.5 8 12l6.5 6.5" />,
  memory: (
    <>
      <path d="M7 3.5h10a1 1 0 0 1 1 1v16l-6-4-6 4v-16a1 1 0 0 1 1-1z" />
      {PIN(12, 8.5)}
    </>
  ),
  chats: (
    <>
      <path d="M4 6.5A2.5 2.5 0 0 1 6.5 4h7A2.5 2.5 0 0 1 16 6.5v4a2.5 2.5 0 0 1-2.5 2.5H8.5L5 16v-3.2A2.5 2.5 0 0 1 4 10.5z" />
      <path d="M19 9.5a2 2 0 0 1 1 1.7v3.8a2 2 0 0 1-1 1.7V19l-3-2.3h-3.5a2.5 2.5 0 0 1-2.2-1.3" />
    </>
  ),
  // The reply styles, from short to long.
  bubble: (
    <>
      <path d="M5 7.5A2.5 2.5 0 0 1 7.5 5h9A2.5 2.5 0 0 1 19 7.5v5a2.5 2.5 0 0 1-2.5 2.5H10l-4 3.5V15a2.5 2.5 0 0 1-1-2z" />
      {PIN(12, 10)}
    </>
  ),
  story: (
    <>
      <path d="M7 3.5h7.5L18 7v12.5a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1v-15a1 1 0 0 1 1-1z" />
      <path d="M9 11h6M9 14.5h6M9 18h3.5" />
    </>
  ),
  book: (
    <>
      <path d="M12 6.5c-2-1.5-4.5-2-8-1.5v13c3.5-.5 6 0 8 1.5 2-1.5 4.5-2 8-1.5V5c-3.5-.5-6 0-8 1.5zM12 6.5V19.5" />
    </>
  ),
  sliders: (
    <>
      <path d="M5 7h14M5 12h14M5 17h14" />
      {PIN(9, 7)}
      <circle cx={15} cy={12} r={1.9} className="icon-dot" />
      <circle cx={8} cy={17} r={1.9} className="icon-dot" />
    </>
  ),
  tone: (
    <>
      <circle cx={12} cy={12} r={8} />
      <path d="M8.5 14.5c1 1.2 2.2 1.8 3.5 1.8s2.5-.6 3.5-1.8" />
      {PIN(9.3, 9.8)}
    </>
  ),
  style: (
    <>
      <path d="M4.5 19.5 5.5 15 15.8 4.7a2.1 2.1 0 0 1 3 3L8.5 18z" />
      <path d="M13.8 6.7l3 3" />
      {PIN(5.2, 18.8)}
    </>
  ),
  download: (
    <>
      <path d="M12 4v10.5M7.5 10 12 14.5 16.5 10M5 19.5h14" />
    </>
  ),
  remove: (
    <>
      <path d="M4.5 7h15M9.5 7V4.5h5V7M6.5 7l1 12.5h9l1-12.5" />
    </>
  ),
  check: <path d="M5 12.5l4.5 4.5L19 7" />,
  refresh: (
    <>
      <path d="M19 12a7 7 0 1 1-2-4.9M19 4.5V8h-3.5" />
    </>
  ),
  gpu: (
    <>
      <rect x={3} y={6.5} width={18} height={11} rx={2.5} />
      <circle cx={9} cy={12} r={2.6} />
      <path d="M14.5 10h3M14.5 14h3M6 17.5v2" />
    </>
  ),
  ram: (
    <>
      <rect x={3} y={7} width={18} height={8} rx={1.8} />
      <path d="M7 15v3M11 15v3M15 15v3M19 15v3M7 10.5h2M11 10.5h2M15 10.5h2" />
    </>
  ),
  disk: (
    <>
      <rect x={4} y={5.5} width={16} height={13} rx={2.5} />
      <path d="M4 13.5h16" />
      {PIN(16.5, 16)}
    </>
  ),
  plus: <path d="M12 5v14M5 12h14" />,
  warning: (
    <>
      <path d="M10.3 4.8a2 2 0 0 1 3.4 0l7 12.2a2 2 0 0 1-1.7 3H5a2 2 0 0 1-1.7-3z" />
      <path d="M12 9.5v4" />
      {PIN(12, 16.8)}
    </>
  ),
  search: (
    <>
      <circle cx="10.5" cy="10.5" r="5.5" />
      <path d="M14.5 14.5 19 19" />
      {PIN(10.5, 10.5)}
    </>
  ),
  import: (
    <>
      <path d="M12 15V4M7.5 8.5 12 4l4.5 4.5M5 14v4.5A1.5 1.5 0 0 0 6.5 20h11a1.5 1.5 0 0 0 1.5-1.5V14" />
    </>
  ),
  edit: (
    <>
      <path d="M4.5 19.5 5.5 15 15.8 4.7a2.1 2.1 0 0 1 3 3L8.5 18z" />
    </>
  ),
  more: (
    <>
      <circle cx={5.5} cy={12} r={1.9} className="icon-dot" />
      {PIN(12, 12)}
      <circle cx={18.5} cy={12} r={1.9} className="icon-dot" />
    </>
  ),
  regenerate: <path d="M19 12a7 7 0 1 1-2-4.9M19 4.5V8h-3.5" />,
  shorter: <path d="M5 8h14M5 12h9M5 16h5" />,
  longer: <path d="M5 7h14M5 11h14M5 15h14M5 19h9" />,
  // The logo figure's head and neck joint.
  inCharacter: (
    <>
      <ellipse cx={12.5} cy={8.5} rx={4.2} ry={5} transform="rotate(20 12.5 8.5)" />
      <path d="M10.5 15.5 9 20.5h6.5" />
      {PIN(10.6, 15.6)}
    </>
  ),
  continue: <path d="M8 5.5v13l10-6.5z" />,
  rewind: <path d="M12 6.5 6 12l6 5.5M18 6.5 12 12l6 5.5" />,
  branch: (
    <>
      <path d="M7 4v16M7 14c0-3.5 2.5-5 6-5h4" />
      <path d="M14.5 6.5 17 9l-2.5 2.5" />
    </>
  ),
  remember: (
    <>
      <path d="M7 3.5h10a1 1 0 0 1 1 1v16l-6-4-6 4v-16a1 1 0 0 1 1-1z" />
      <path d="M12 7.5v5M9.5 10h5" />
    </>
  ),
  pin: (
    <>
      <path d="M12 14v6.5" />
      {PIN(12, 9)}
      <circle cx={12} cy={9} r={4.8} />
    </>
  ),
  hide: (
    <>
      <path d="M3.5 12s3-6 8.5-6 8.5 6 8.5 6-3 6-8.5 6-8.5-6-8.5-6z" />
      <path d="M4.5 4.5 19.5 19.5" />
    </>
  ),
  show: (
    <>
      <path d="M3.5 12s3-6 8.5-6 8.5 6 8.5 6-3 6-8.5 6-8.5-6-8.5-6z" />
      {PIN(12, 12)}
    </>
  ),
  star: (
    <>
      <path d="M12 4.5l2.3 4.7 5.2.8-3.8 3.7.9 5.2L12 16.4l-4.6 2.5.9-5.2-3.8-3.7 5.2-.8z" />
    </>
  ),
  close: <path d="M6 6l12 12M18 6 6 18" />,
  send: (
    <>
      <path d="M4.5 11.5 19.5 4.5l-5 15-3-6.5z" />
      <path d="M11.5 13 19.5 4.5" />
    </>
  ),
  stop: <rect x={6.5} y={6.5} width={11} height={11} rx={2} />,
  picture: (
    <>
      <rect x={4} y={5} width={16} height={14} rx={2} />
      <path d="m4.5 16.5 4.5-4.5 3.5 3.5 2.5-2.5 4.5 4.5" />
      <circle cx={15.5} cy={9.5} r={1.5} />
    </>
  ),
  link: (
    <>
      <path d="M10 14 14 10M9 7.5l1.6-1.6a3.5 3.5 0 0 1 5 5L14 12.5M15 16.5l-1.6 1.6a3.5 3.5 0 0 1-5-5L10 11.5" />
    </>
  ),
} satisfies Record<string, ReactNode>;

export type IconName = keyof typeof ICONS;

const BY_NAME = new Map(Object.entries(ICONS) as [IconName, ReactNode][]);

/**
 * An icon. Next to a text label it is hidden from screen readers. Alone in a
 * button, the button needs its own `aria-label`.
 */
export function Icon({ name, className }: { name: IconName; className?: string }) {
  return (
    <svg
      className={className ? `icon ${className}` : "icon"}
      viewBox="0 0 24 24"
      aria-hidden="true"
      focusable="false"
    >
      {BY_NAME.get(name)}
    </svg>
  );
}
