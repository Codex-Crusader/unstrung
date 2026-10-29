// The --self-test run: real commands through the real IPC path, then a report.
// The core writes the report to logs/self-test.json and exits.

import type { Screen } from "./App";
import {
  deleteCharacter,
  getStatus,
  listCharacters,
  onStatus,
  openCharacter,
  regenerate,
  saveCharacter,
  selfTestReport,
  sendMessage,
  setChatStyle,
  setChatTone,
  stageUpload,
  testReply,
  toUiError,
  type ChatView,
  type EngineStatus,
  type ReplyHandler,
  type SelfTestReport,
} from "./api";

function waitForReady(): Promise<EngineStatus> {
  return new Promise((resolve) => {
    const settle = (status: EngineStatus) => {
      if (status.state === "ready" || status.state === "error") {
        resolve(status);
      }
    };
    void onStatus(settle);
    void getStatus().then(settle);
  });
}

interface Streamed {
  events: number;
  view: ChatView | null;
  error: string;
}

/** Runs one streamed reply command and waits for its end. */
function stream(run: (handler: ReplyHandler) => Promise<null>): Promise<Streamed> {
  return new Promise((resolve) => {
    let events = 0;
    run((event) => {
      if (event.kind === "text") events += 1;
      if (event.kind === "done") resolve({ events, view: event.view, error: "" });
      if (event.kind === "failed") resolve({ events, view: null, error: event.error.detail });
    }).catch((e: unknown) => {
      resolve({ events, view: null, error: toUiError(e).detail });
    });
  });
}

const SHOW_MS = 1500;

function pause(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/** A small PNG drawn in the window, to send through the real upload path. */
async function drawPicture(): Promise<Uint8Array> {
  const canvas = document.createElement("canvas");
  canvas.width = 300;
  canvas.height = 200;
  const g = canvas.getContext("2d");
  if (!g) throw new Error("no 2d canvas");
  g.fillStyle = "#a87a2c";
  g.fillRect(0, 0, 300, 200);
  g.fillStyle = "#2a2622";
  g.beginPath();
  g.arc(150, 100, 60, 0, Math.PI * 2);
  g.fill();
  const blob = await new Promise<Blob | null>((resolve) => {
    canvas.toBlob(resolve, "image/png");
  });
  if (!blob) throw new Error("the canvas gave no PNG");
  return new Uint8Array(await blob.arrayBuffer());
}

/** Phase 2 done-test path: make a character with a picture, then chat with it. */
async function checkCreator(show: (screen: Screen) => void, renId: number): Promise<string> {
  // Each screen stays for a moment, so a screenshot of the run can show it.
  show({ name: "creator", characterId: renId });
  await pause(SHOW_MS);
  show({ name: "personas" });
  await pause(SHOW_MS);
  const staged = await stageUpload(await drawPicture());
  if (staged.width !== 300 || staged.height !== 200) return `staged size ${String(staged.width)}x${String(staged.height)}`;
  const fields = {
    name: "Self Test Iris",
    tagline: "A test character.",
    personality: "Calm and kind. She talks in short, warm sentences.",
    scenario: "",
    greeting: "Hello. I am Iris. What brings you here?",
    example_dialogue: "",
    voice_anchor: "",
    appearance_tags: "",
    default_tone: "wholesome",
    default_reply_style: "conversational",
    age: 30,
    is_adult: false,
  };
  const character = await saveCharacter(null, fields, {
    kind: "staged",
    token: staged.token,
    crop: { x: 50, y: 0, size: 200 },
  });
  if (!character.avatar_path) return "the picture was not saved";
  show({ name: "chat", characterId: character.id });
  const view = await openCharacter(character.id);
  if (view.messages[0]?.content !== fields.greeting) return "the new chat does not start with the greeting";
  const sent = await stream((h) => sendMessage(view.chat.id, "Hi Iris. How are you today?", h));
  if (sent.error || !sent.view) return `the new character did not reply: ${sent.error}`;

  const test = await new Promise<string>((resolve) => {
    testReply(fields, null, [{ role: "character", content: fields.greeting }, { role: "user", content: "Hi!" }], (e) => {
      if (e.kind === "done") resolve(e.text ? "" : "the test chat reply was empty");
      if (e.kind === "failed") resolve(`test chat failed: ${e.error.detail}`);
    }).catch((e: unknown) => {
      resolve(`test chat failed: ${toUiError(e).detail}`);
    });
  });
  if (test) return test;

  await deleteCharacter(character.id);
  const left = await listCharacters();
  if (left.some((c) => c.id === character.id)) return "the character was not deleted";
  return "";
}

async function check(report: SelfTestReport, show: (screen: Screen) => void): Promise<string> {
  const characters = await listCharacters();
  const ren = characters.find((c) => c.name === "Ren Kagami") ?? characters[0];
  if (!ren) return "no characters";
  show({ name: "chat", characterId: ren.id });
  const status = await waitForReady();
  if (status.state !== "ready") return `engine: ${JSON.stringify(status)}`;
  let view = await openCharacter(ren.id);
  report.messages_at_open = view.messages.length;
  const chatId = view.chat.id;
  await setChatTone(chatId, "dark", "");
  view = await setChatStyle(chatId, "texting", null);
  if (view.chat.reply_style !== "texting") return "the reply style did not change";

  const sent = await stream((h) => sendMessage(chatId, "Hello. Who are you, and what is this place?", h));
  report.text_events = sent.events;
  if (sent.error || !sent.view) return `reply failed: ${sent.error}`;
  if (sent.events < 2) return "the reply did not stream";
  view = sent.view;
  report.messages_after = view.messages.length;
  if (report.messages_after !== report.messages_at_open + 2) return "the message and the reply were not both saved";
  const reply = view.messages.at(-1);
  if (reply?.role !== "character" || reply.id !== view.chat.head_message_id) {
    return "the saved reply is not the head of the chat";
  }

  const again = await stream((h) => regenerate(chatId, reply.id, "longer", h));
  if (again.error || !again.view) return `regenerate failed: ${again.error}`;
  const versions = again.view.messages.at(-1)?.siblings ?? [];
  if (versions.length < 2 || !versions.includes(reply.id)) return "regenerate did not keep the old version";
  const creator = await checkCreator(show, ren.id);
  if (creator) return creator;
  return `ok: ${reply.content}`;
}

/** `show` opens the chat screen of the tested character, so a screenshot can show it. */
export async function runSelfTest(show: (screen: Screen) => void): Promise<void> {
  const report: SelfTestReport = { ok: false, detail: "", messages_at_open: 0, messages_after: 0, text_events: 0 };
  try {
    const result = await check(report, show);
    report.ok = result.startsWith("ok: ");
    report.detail = result;
  } catch (e) {
    report.detail = toUiError(e).detail;
  } finally {
    await selfTestReport(report);
  }
}
