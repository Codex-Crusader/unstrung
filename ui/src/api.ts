// Typed wrappers for the core commands. The UI has no other way to reach
// the disk, the network or the engines.

import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type EngineStatus =
  | { state: "starting" }
  | { state: "loading_model"; model: string }
  | { state: "ready"; model: string }
  | { state: "error"; message: string; detail: string };

export type Role = "user" | "character" | "system" | "ooc";

export interface Message {
  id: number;
  chat_id: number;
  parent_id: number | null;
  role: Role;
  content: string;
  model: string | null;
  created_at: number;
}

export interface Character {
  id: number;
  name: string;
  avatar_path: string | null;
  tagline: string;
}

export interface Chat {
  id: number;
  character_id: number;
  title: string;
  tone: string;
  reply_style: string;
  chat_model: string | null;
  head_message_id: number | null;
}

export interface ChatView {
  chat: Chat;
  character: Character;
  messages: Message[];
}

export interface UiError {
  message: string;
  detail: string;
}

export type ReplyEvent =
  | { kind: "user_saved"; message: Message }
  | { kind: "text"; text: string }
  | { kind: "done"; message: Message | null }
  | { kind: "failed"; error: UiError };

export function isUiError(value: unknown): value is UiError {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as UiError).message === "string" &&
    typeof (value as UiError).detail === "string"
  );
}

/** Any thrown value as a UiError. */
export function toUiError(value: unknown): UiError {
  if (isUiError(value)) return value;
  return { message: "Something went wrong.", detail: String(value) };
}

export const getStatus = () => invoke<EngineStatus>("get_status");

export function onStatus(handler: (status: EngineStatus) => void): Promise<UnlistenFn> {
  return listen<EngineStatus>("engine-status", (event) => {
    handler(event.payload);
  });
}

export const openChat = () => invoke<ChatView>("open_chat");

function channel(handler: (event: ReplyEvent) => void): Channel<ReplyEvent> {
  const ch = new Channel<ReplyEvent>();
  ch.onmessage = handler;
  return ch;
}

export const sendMessage = (chatId: number, text: string, handler: (event: ReplyEvent) => void) =>
  invoke<null>("send_message", { chatId, text, onEvent: channel(handler) });

export const retryReply = (chatId: number, handler: (event: ReplyEvent) => void) =>
  invoke<null>("retry_reply", { chatId, onEvent: channel(handler) });

export const stopReply = () => invoke<null>("stop_reply");

export const restartEngine = () => invoke<null>("restart_engine");

export const selfTestMode = () => invoke<boolean>("self_test_mode");

export interface SelfTestReport {
  ok: boolean;
  detail: string;
  messages_at_open: number;
  messages_after: number;
  text_events: number;
}

export const selfTestReport = (report: SelfTestReport) => invoke<null>("self_test_report", { report });

/** The URL of a character's profile picture, served by the core. */
export function avatarUrl(characterId: number): string {
  const windows = navigator.userAgent.includes("Windows");
  const base = windows ? "http://avatar.localhost" : "avatar://localhost";
  return `${base}/character/${String(characterId)}`;
}
