// The --self-test run: real commands through the real IPC path, then a report.
// The core writes the report to logs/self-test.json and exits.

import {
  getStatus,
  listCharacters,
  onStatus,
  openCharacter,
  regenerate,
  selfTestReport,
  sendMessage,
  setChatStyle,
  setChatTone,
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

async function check(report: SelfTestReport, show: (characterId: number) => void): Promise<string> {
  const characters = await listCharacters();
  const ren = characters.find((c) => c.name === "Ren Kagami") ?? characters[0];
  if (!ren) return "no characters";
  show(ren.id);
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
  return `ok: ${reply.content}`;
}

/** `show` opens the chat screen of the tested character, so a screenshot can show it. */
export async function runSelfTest(show: (characterId: number) => void): Promise<void> {
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
