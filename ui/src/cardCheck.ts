// The character card check: tips while the user writes a card, never errors.

import type { CharacterFields } from "./api";
import { strings } from "./strings";

/** The same estimate as the core: about 3.5 characters per token. */
export function estimateTokens(text: string): number {
  return Math.ceil(text.length / 3.5);
}

/** True when filled-in Looks do not start with woman, man or person. Pictures
 * from tags need that word, or they can show the wrong kind of person. */
export function needsPersonWord(looks: string): boolean {
  const first = (looks.split(",")[0] ?? "").trim().toLowerCase();
  return looks.trim() !== "" && !["woman", "man", "person"].includes(first);
}

/** The card part of the prompt: 20% of an 8192-token context. */
export const CARD_BUDGET = Math.floor((8192 * 20) / 100);

/** Greeting length in words that fits each reply style. */
const GREETING_WORDS = new Map<string, [number, number]>([
  ["texting", [1, 35]],
  ["conversational", [8, 110]],
  ["storyteller", [40, 260]],
  ["novel", [90, 600]],
]);

const USER_ACTION = /\{\{user\}\}\s+(feels|felt|is|was|wants|loves|hates|thinks|nods|smiles|says|said|looks|blushes|agrees)\b/i;

function words(text: string): number {
  return text.trim() === "" ? 0 : text.trim().split(/\s+/).length;
}

/** True when the personality is only a list of short traits. */
function onlyTraits(personality: string): boolean {
  const text = personality.trim();
  if (text === "" || /[.!?]/.test(text)) return false;
  const parts = text.split(/[,;\n]+/).filter((p) => p.trim() !== "");
  return parts.length >= 2 && parts.every((p) => words(p) <= 3);
}

export interface Tip {
  key: string;
  text: string;
}

export function checkCard(f: CharacterFields): { tips: Tip[]; tokens: number } {
  const tips: Tip[] = [];
  const tokens = estimateTokens(
    [f.personality, f.scenario, f.appearance_tags, f.outfit, f.example_dialogue].join("\n"),
  );
  if (onlyTraits(f.personality)) {
    tips.push({ key: "traits", text: strings.check.traits });
  }
  if (f.example_dialogue.trim() === "") {
    tips.push({ key: "examples", text: strings.check.examples });
  }
  if (tokens > CARD_BUDGET) {
    tips.push({ key: "budget", text: strings.check.budget(tokens, CARD_BUDGET) });
  }
  if ([f.personality, f.scenario, f.greeting].some((t) => USER_ACTION.test(t))) {
    tips.push({ key: "user", text: strings.check.user });
  }
  const range = GREETING_WORDS.get(f.default_reply_style);
  const greeting = words(f.greeting);
  if (range && greeting > 0 && (greeting < range[0] || greeting > range[1])) {
    tips.push({ key: "greeting", text: strings.check.greeting });
  }
  return { tips, tokens };
}
