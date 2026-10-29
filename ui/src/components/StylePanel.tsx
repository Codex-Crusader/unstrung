import { useState } from "react";
import type { Actions, Description, Initiative, Length, Perspective, StyleSettings, Talk } from "../api";
import { strings } from "../strings";

const PRESETS = ["texting", "conversational", "storyteller", "novel", "custom"] as const;

const CONVERSATIONAL: StyleSettings = {
  length: "short",
  talk: "mostly_dialogue",
  description: "some",
  initiative: "balanced",
  perspective: "third",
  actions: "asterisks",
};

/** The same values as `StyleSettings::of` in the core, to start the custom sliders. */
const PRESET_SETTINGS = new Map<string, StyleSettings>([
  [
    "texting",
    {
      length: "very_short",
      talk: "dialogue_only",
      description: "minimal",
      initiative: "follow_me",
      perspective: "third",
      actions: "asterisks",
    },
  ],
  ["conversational", CONVERSATIONAL],
  [
    "storyteller",
    {
      length: "medium",
      talk: "mixed",
      description: "some",
      initiative: "balanced",
      perspective: "third",
      actions: "asterisks",
    },
  ],
  [
    "novel",
    {
      length: "long",
      talk: "mostly_narration",
      description: "vivid",
      initiative: "lead_the_story",
      perspective: "third",
      actions: "plain",
    },
  ],
]);

interface Slider {
  label: string;
  values: readonly string[];
  get: (s: StyleSettings) => string;
  set: (s: StyleSettings, value: string) => StyleSettings;
}

const SLIDERS: Slider[] = [
  {
    label: strings.sliders.length,
    values: ["very_short", "short", "medium", "long", "very_long"],
    get: (s) => s.length,
    set: (s, v) => ({ ...s, length: v as Length }),
  },
  {
    label: strings.sliders.talk,
    values: ["dialogue_only", "mostly_dialogue", "mixed", "mostly_narration"],
    get: (s) => s.talk,
    set: (s, v) => ({ ...s, talk: v as Talk }),
  },
  {
    label: strings.sliders.description,
    values: ["minimal", "some", "vivid"],
    get: (s) => s.description,
    set: (s, v) => ({ ...s, description: v as Description }),
  },
  {
    label: strings.sliders.initiative,
    values: ["follow_me", "balanced", "lead_the_story"],
    get: (s) => s.initiative,
    set: (s, v) => ({ ...s, initiative: v as Initiative }),
  },
  {
    label: strings.sliders.perspective,
    values: ["first", "third"],
    get: (s) => s.perspective,
    set: (s, v) => ({ ...s, perspective: v as Perspective }),
  },
  {
    label: strings.sliders.actions,
    values: ["asterisks", "plain"],
    get: (s) => s.actions,
    set: (s, v) => ({ ...s, actions: v as Actions }),
  },
];

/** Reads the chat's saved custom style. Returns null when it is missing or not valid. */
function parseCustom(json: string): StyleSettings | null {
  try {
    const value: unknown = JSON.parse(json);
    if (typeof value !== "object" || value === null) return null;
    const candidate = { ...CONVERSATIONAL, ...(value as Partial<StyleSettings>) };
    return SLIDERS.every((s) => s.values.includes(s.get(candidate))) ? candidate : null;
  } catch {
    return null;
  }
}

export function StylePanel({
  style,
  customJson,
  onApply,
  onClose,
}: {
  style: string;
  customJson: string;
  onApply: (style: string, custom: StyleSettings | null) => void;
  onClose: () => void;
}) {
  const [picked, setPicked] = useState(style);
  const [custom, setCustom] = useState<StyleSettings>(
    () => parseCustom(customJson) ?? PRESET_SETTINGS.get(style) ?? CONVERSATIONAL,
  );

  return (
    <div className="popover style-panel" role="dialog" aria-label={strings.header.style}>
      <fieldset className="preset-list">
        <legend>{strings.header.style}</legend>
        {PRESETS.map((p) => (
          <label key={p} className={p === picked ? "preset preset-picked" : "preset"}>
            <input
              type="radio"
              name="reply-style"
              value={p}
              checked={p === picked}
              onChange={() => {
                setPicked(p);
                const base = PRESET_SETTINGS.get(p);
                if (base) setCustom(base);
              }}
            />
            <span className="preset-name">{strings.styles.get(p)}</span>
            <span className="preset-hint">{strings.styleHints.get(p)}</span>
          </label>
        ))}
      </fieldset>
      {picked === "custom" && (
        <div className="sliders">
          {SLIDERS.map((s) => (
            <label key={s.label} className="slider-row">
              <span>{s.label}</span>
              <select
                value={s.get(custom)}
                onChange={(e) => {
                  setCustom(s.set(custom, e.target.value));
                }}
              >
                {s.values.map((v) => (
                  <option key={v} value={v}>
                    {strings.sliderValues.get(v) ?? v}
                  </option>
                ))}
              </select>
            </label>
          ))}
        </div>
      )}
      <div className="popover-actions">
        <button type="button" className="button button-quiet" onClick={onClose}>
          {strings.message.cancel}
        </button>
        <button
          type="button"
          className="button"
          onClick={() => {
            onApply(picked, picked === "custom" ? custom : null);
          }}
        >
          {strings.header.apply}
        </button>
      </div>
    </div>
  );
}
