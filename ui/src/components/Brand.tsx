import { useRef, useState, type PointerEvent } from "react";
import { strings } from "../strings";
import figure from "../assets/figure.png";
// From myinstants.com ("long brain fart"), added by the project owner.
import sound from "../assets/unstrung.mp3";

/** "unstrung" in Morse code (u n s t r u n g), letters run together: a tap is a dot, a hold is a dash. */
const SECRET = ["..-", "-.", "...", "-", ".-.", "..-", "-.", "--."].join("");
/** A press this long or longer is a dash. */
const DASH_MS = 250;
/** A pause this long starts the code again. */
const RESET_MS = 2000;

/** The logo in the sidebar. Tap "unstrung" in Morse code on it. */
export function Brand() {
  const taps = useRef({ code: "", down: 0, last: 0 });
  const [playing, setPlaying] = useState(false);

  const down = (e: PointerEvent) => {
    if (e.button !== 0) return;
    taps.current.down = e.timeStamp;
  };
  const up = (e: PointerEvent) => {
    const t = taps.current;
    if (e.button !== 0 || t.down === 0) return;
    if (e.timeStamp - t.last > RESET_MS) t.code = "";
    t.code = (t.code + (e.timeStamp - t.down >= DASH_MS ? "-" : ".")).slice(-SECRET.length);
    t.last = e.timeStamp;
    t.down = 0;
    if (t.code === SECRET && !playing) {
      t.code = "";
      const audio = new Audio(sound);
      setPlaying(true);
      audio.onended = () => {
        setPlaying(false);
      };
      audio.play().catch(() => {
        setPlaying(false);
      });
    }
  };

  return (
    <span className="brand" onPointerDown={down} onPointerUp={up}>
      <img className={playing ? "brand-figure brand-figure-loose" : "brand-figure"} src={figure} alt="" draggable={false} />
      {strings.appName}
    </span>
  );
}
