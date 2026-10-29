// The --self-test run: one message through the real command path, then a report.
// The core writes the report to logs/self-test.json and exits.

import {
  getStatus,
  onStatus,
  openChat,
  selfTestReport,
  sendMessage,
  toUiError,
  type EngineStatus,
  type Message,
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

function send(chatId: number, text: string): Promise<{ events: number; reply: Message | null; error: string }> {
  return new Promise((resolve) => {
    let events = 0;
    sendMessage(chatId, text, (event) => {
      if (event.kind === "text") events += 1;
      if (event.kind === "done") resolve({ events, reply: event.message, error: "" });
      if (event.kind === "failed") resolve({ events, reply: null, error: event.error.detail });
    }).catch((e: unknown) => {
      resolve({ events, reply: null, error: toUiError(e).detail });
    });
  });
}

export async function runSelfTest(): Promise<void> {
  const report: SelfTestReport = { ok: false, detail: "", messages_at_open: 0, messages_after: 0, text_events: 0 };
  try {
    const status = await waitForReady();
    if (status.state !== "ready") {
      report.detail = `engine: ${JSON.stringify(status)}`;
      return;
    }
    const view = await openChat();
    report.messages_at_open = view.messages.length;
    const result = await send(view.chat.id, "Hello. Who are you, and what is this place?");
    report.text_events = result.events;
    const after = await openChat();
    report.messages_after = after.messages.length;
    const last = after.messages.at(-1);
    if (result.error) {
      report.detail = `reply failed: ${result.error}`;
    } else if (result.events < 2) {
      report.detail = "the reply did not stream";
    } else if (report.messages_after !== report.messages_at_open + 2) {
      report.detail = "the message and the reply were not both saved";
    } else if (!result.reply || last?.id !== result.reply.id) {
      report.detail = "the saved reply is not the head of the chat";
    } else {
      report.ok = true;
      report.detail = result.reply.content;
    }
  } catch (e) {
    report.detail = toUiError(e).detail;
  } finally {
    await selfTestReport(report);
  }
}
