import { useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import { strings } from "../strings";
import figure from "../assets/figure.png";

/** The game area in canvas units. The canvas scales it to its box. */
const W = 360;
const H = 220;
const LANES = 5;
const BAR_Y = 12;
/** A string that reaches this line reaches the puppet. */
const GROUND = H - 58;
const LIVES = 3;

interface Str {
  lane: number;
  /** How far the string hangs down from the bar. */
  len: number;
  frayed: boolean;
  /** Where it was snipped, or null. The cut piece falls from there. */
  cut: number | null;
  fall: number;
}

interface Game {
  strings: Str[];
  snipped: number;
  kept: number;
  lives: number;
  time: number;
  nextSpawn: number;
  /** Seconds of stumble after a tangle. */
  stumble: number;
}

interface Hud {
  snipped: number;
  kept: number;
  lives: number;
  over: boolean;
}

const laneX = (lane: number) => (W / LANES) * (lane + 0.5);

function newGame(): Game {
  return { strings: [], snipped: 0, kept: 0, lives: LIVES, time: 0, nextSpawn: 0.6, stumble: 0 };
}

/** Moves the game on by `dt` seconds. */
function step(g: Game, dt: number) {
  g.time += dt;
  g.stumble = Math.max(0, g.stumble - dt);
  // Faster and busier as the score grows, up to a limit.
  const level = Math.min(1, (g.snipped + g.kept) / 40);
  const speed = 40 + 70 * level;
  if (g.time >= g.nextSpawn) {
    const free = [...Array(LANES).keys()].filter((l) => !g.strings.some((s) => s.lane === l && s.cut === null));
    const lane = free[Math.floor(Math.random() * free.length)];
    if (lane !== undefined) g.strings.push({ lane, len: 0, frayed: Math.random() < 0.5, cut: null, fall: 0 });
    g.nextSpawn = g.time + 1.4 - 0.8 * level + Math.random() * 0.4;
  }
  for (const s of g.strings) {
    if (s.cut === null) s.len += speed * dt;
    else s.fall += 260 * dt;
  }
  g.strings = g.strings.filter((s) => {
    if (s.cut !== null) return s.cut + s.fall < H + 40;
    if (BAR_Y + s.len < GROUND) return true;
    if (s.frayed) {
      g.lives -= 1;
      g.stumble = 0.6;
    } else {
      g.kept += 1;
    }
    return false;
  });
}

/** Snips the string nearest to `x` at height `y`. Snipping a clean string costs a life. */
function snip(g: Game, x: number, y: number) {
  const hit = g.strings
    .filter((s) => s.cut === null && Math.abs(laneX(s.lane) - x) < W / LANES / 2 && y <= BAR_Y + s.len + 16)
    .sort((a, b) => Math.abs(laneX(a.lane) - x) - Math.abs(laneX(b.lane) - x))[0];
  if (!hit) return;
  hit.cut = Math.max(BAR_Y + 4, Math.min(y, BAR_Y + hit.len - 4));
  if (hit.frayed) {
    g.snipped += 1;
  } else {
    g.lives -= 1;
    g.stumble = 0.6;
  }
}

function draw(ctx: CanvasRenderingContext2D, g: Game, img: HTMLImageElement, colors: Record<string, string>) {
  ctx.clearRect(0, 0, W, H);
  ctx.lineCap = "round";
  ctx.strokeStyle = colors.line ?? "#888";
  ctx.lineWidth = 4;
  ctx.beginPath();
  ctx.moveTo(8, BAR_Y);
  ctx.lineTo(W - 8, BAR_Y);
  ctx.stroke();
  ctx.strokeStyle = colors.soft ?? "#888";
  ctx.setLineDash([4, 6]);
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(0, GROUND);
  ctx.lineTo(W, GROUND);
  ctx.stroke();
  ctx.setLineDash([]);

  for (const s of g.strings) {
    const x = laneX(s.lane);
    const tip = BAR_Y + s.len;
    const color = (s.frayed ? colors.bad : colors.soft) ?? "#888";
    ctx.strokeStyle = color;
    ctx.lineWidth = 2;
    const piece = (from: number, to: number, dy: number) => {
      ctx.beginPath();
      if (s.frayed) {
        // A frayed string kinks from side to side; a clean one hangs straight.
        for (let y = from; y <= to; y += 6) {
          const dx = ((y / 6) % 2 < 1 ? -2 : 2) * Math.min(1, (y - BAR_Y) / 30);
          if (y === from) ctx.moveTo(x + dx, y + dy);
          else ctx.lineTo(x + dx, y + dy);
        }
      } else {
        ctx.moveTo(x, from + dy);
        ctx.lineTo(x, to + dy);
      }
      ctx.stroke();
    };
    if (s.cut === null) {
      piece(BAR_Y, tip, 0);
    } else {
      piece(BAR_Y, s.cut, 0);
      ctx.globalAlpha = Math.max(0, 1 - s.fall / 160);
      piece(s.cut, tip, s.fall);
      ctx.globalAlpha = 1;
    }
    const end = s.cut === null ? tip : tip + s.fall;
    if (s.frayed) {
      // Loose fibers at the end.
      for (const dx of [-5, 0, 5]) {
        ctx.beginPath();
        ctx.moveTo(x, end);
        ctx.lineTo(x + dx, end + 7);
        ctx.stroke();
      }
    } else {
      ctx.fillStyle = colors.accent ?? "#c9a052";
      ctx.beginPath();
      ctx.arc(x, end, 4, 0, Math.PI * 2);
      ctx.fill();
    }
  }

  // The puppet runs back and forth along the bottom, and wobbles after a tangle.
  const size = 44;
  const px = W / 2 + Math.sin(g.time * 0.9) * (W / 2 - size);
  const bob = Math.abs(Math.sin(g.time * 9)) * 3;
  ctx.save();
  ctx.translate(px, H - 8 - size / 2 - bob);
  if (Math.cos(g.time * 0.9) < 0) ctx.scale(-1, 1);
  if (g.stumble > 0) ctx.rotate(Math.sin(g.stumble * 30) * 0.4);
  if (img.complete) ctx.drawImage(img, -size / 2, -size / 2, size, size);
  ctx.restore();
}

/** "Snip the strings": a small game to play while a download runs. */
export function SnipGame() {
  const s = strings.snip;
  const [open, setOpen] = useState(false);
  const [round, setRound] = useState(0);
  const [hud, setHud] = useState<Hud>({ snipped: 0, kept: 0, lives: LIVES, over: false });
  const [best, setBest] = useState(0);
  const canvas = useRef<HTMLCanvasElement>(null);
  const game = useRef<Game>(newGame());

  useEffect(() => {
    const el = canvas.current;
    const ctx = el?.getContext("2d");
    if (!open || !el || !ctx) return;
    const dpr = window.devicePixelRatio || 1;
    el.width = W * dpr;
    el.height = H * dpr;
    ctx.scale(dpr, dpr);
    const css = getComputedStyle(el);
    const colors = {
      line: css.getPropertyValue("--panel-line"),
      soft: css.getPropertyValue("--text-soft"),
      bad: css.getPropertyValue("--bad"),
      accent: css.getPropertyValue("--accent"),
    };
    const img = new Image();
    img.src = figure;
    const g = newGame();
    game.current = g;
    setHud({ snipped: 0, kept: 0, lives: LIVES, over: false });
    let last = performance.now();
    let frame = 0;
    const tick = (now: number) => {
      // A hidden window pauses the loop; do not jump ahead when it comes back.
      step(g, Math.min(0.05, (now - last) / 1000));
      last = now;
      draw(ctx, g, img, colors);
      const over = g.lives <= 0;
      setHud((h) =>
        h.snipped === g.snipped && h.kept === g.kept && h.lives === g.lives && h.over === over
          ? h
          : { snipped: g.snipped, kept: g.kept, lives: Math.max(0, g.lives), over },
      );
      if (over) {
        setBest((b) => Math.max(b, g.snipped + g.kept));
        return;
      }
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => {
      cancelAnimationFrame(frame);
    };
  }, [open, round]);

  const tap = (e: PointerEvent<HTMLCanvasElement>) => {
    const box = e.currentTarget.getBoundingClientRect();
    snip(game.current, ((e.clientX - box.left) / box.width) * W, ((e.clientY - box.top) / box.height) * H);
  };
  const key = (e: KeyboardEvent<HTMLCanvasElement>) => {
    const lane = Number(e.key) - 1;
    if (!Number.isInteger(lane) || lane < 0 || lane >= LANES) return;
    e.preventDefault();
    const str = game.current.strings.find((x) => x.lane === lane && x.cut === null);
    if (str) snip(game.current, laneX(lane), BAR_Y + str.len / 2);
  };

  if (!open) {
    return (
      <p className="snip-open">
        <button
          type="button"
          className="link-button"
          onClick={() => {
            setOpen(true);
          }}
        >
          {s.open}
        </button>
      </p>
    );
  }
  return (
    <div className="snip">
      <div className="snip-hud" aria-live="polite">
        <span>{s.score(hud.snipped, hud.kept)}</span>
        <span>{s.lives(hud.lives)}</span>
      </div>
      <div className="snip-board">
        <canvas
          ref={canvas}
          className="snip-canvas"
          tabIndex={0}
          aria-label={s.help}
          onPointerDown={tap}
          onKeyDown={key}
        />
        {hud.over && (
          <div className="snip-over" role="status">
            <p>{s.over(hud.snipped, hud.kept, best)}</p>
            <button
              type="button"
              className="button"
              onClick={() => {
                setRound((r) => r + 1);
              }}
            >
              {s.again}
            </button>
          </div>
        )}
      </div>
      <p className="field-hint">
        {s.help}{" "}
        <button
          type="button"
          className="link-button"
          onClick={() => {
            setOpen(false);
          }}
        >
          {s.close}
        </button>
      </p>
    </div>
  );
}
