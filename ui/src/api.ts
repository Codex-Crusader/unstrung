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
  persona_id: number | null;
  content: string;
  flagged_out_of_character: boolean;
  model: string | null;
  created_at: number;
}

export interface ViewMessage extends Message {
  /** This message and its other versions, oldest first. */
  siblings: number[];
}

export interface Character {
  id: number;
  name: string;
  avatar_path: string | null;
  tagline: string;
  personality: string;
  scenario: string;
  greeting: string;
  example_dialogue: string;
  voice_anchor: string;
  appearance_tags: string;
  default_tone: string;
  default_reply_style: string;
  model_override: string | null;
  age: number;
  is_adult: boolean;
  created_at: number;
}

export interface Persona {
  id: number;
  name: string;
  avatar_path: string | null;
  description: string;
  appearance_tags: string;
  age: number | null;
  is_default: boolean;
}

export interface Chat {
  id: number;
  character_id: number;
  persona_id: number | null;
  title: string;
  tone: string;
  tone_custom: string;
  reply_style: string;
  reply_custom: string;
  chat_model: string | null;
  head_message_id: number | null;
  created_at: number;
  updated_at: number;
}

export interface ChatSummary {
  id: number;
  title: string;
  preview: string;
  message_count: number;
  updated_at: number;
}

export interface ChatView {
  chat: Chat;
  character: Character;
  persona: Persona | null;
  messages: ViewMessage[];
  /** Hidden branches that continue from the last message. */
  branches: number[];
}

export type Length = "very_short" | "short" | "medium" | "long" | "very_long";
export type Talk = "dialogue_only" | "mostly_dialogue" | "mixed" | "mostly_narration";
export type Description = "minimal" | "some" | "vivid";
export type Initiative = "follow_me" | "balanced" | "lead_the_story";
export type Perspective = "first" | "third";
export type Actions = "asterisks" | "plain";

export interface StyleSettings {
  length: Length;
  talk: Talk;
  description: Description;
  initiative: Initiative;
  perspective: Perspective;
  actions: Actions;
}

export type RegenMode = "normal" | "shorter" | "longer" | "in_character" | "not_in_character";

export interface DeleteUndo {
  message_id: number;
  stamp: number;
  old_head: number | null;
}

export interface UiError {
  message: string;
  detail: string;
}

export type ReplyEvent =
  | { kind: "user_saved"; message: Message }
  | { kind: "text"; text: string }
  | { kind: "restart" }
  | { kind: "done"; view: ChatView; hint: string | null }
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

export type ReplyHandler = (event: ReplyEvent) => void;

function channel(handler: ReplyHandler): Channel<ReplyEvent> {
  const ch = new Channel<ReplyEvent>();
  ch.onmessage = handler;
  return ch;
}

export const listCharacters = () => invoke<Character[]>("list_characters");
export const listPersonas = () => invoke<Persona[]>("list_personas");

export const openCharacter = (characterId: number) => invoke<ChatView>("open_character", { characterId });
export const openChat = (chatId: number) => invoke<ChatView>("open_chat", { chatId });
export const newChat = (characterId: number, personaId: number | null) =>
  invoke<ChatView>("new_chat", { characterId, personaId });
export const listChats = (characterId: number) => invoke<ChatSummary[]>("list_chats", { characterId });
export const deleteChat = (chatId: number) => invoke<null>("delete_chat", { chatId });

export const sendMessage = (chatId: number, text: string, handler: ReplyHandler) =>
  invoke<null>("send_message", { chatId, text, onEvent: channel(handler) });
export const editMessage = (chatId: number, messageId: number, text: string, handler: ReplyHandler) =>
  invoke<null>("edit_message", { chatId, messageId, text, onEvent: channel(handler) });
export const regenerate = (chatId: number, messageId: number, mode: RegenMode, handler: ReplyHandler) =>
  invoke<null>("regenerate", { chatId, messageId, mode, onEvent: channel(handler) });
export const continueReply = (chatId: number, handler: ReplyHandler) =>
  invoke<null>("continue_reply", { chatId, onEvent: channel(handler) });
export const retryReply = (chatId: number, handler: ReplyHandler) =>
  invoke<null>("retry_reply", { chatId, onEvent: channel(handler) });
export const stopReply = () => invoke<null>("stop_reply");

export const editReply = (chatId: number, messageId: number, text: string) =>
  invoke<ChatView>("edit_reply", { chatId, messageId, text });
export const rewind = (chatId: number, messageId: number) => invoke<ChatView>("rewind", { chatId, messageId });
export const showBranch = (chatId: number, messageId: number) =>
  invoke<ChatView>("show_branch", { chatId, messageId });
export const deleteMessage = (chatId: number, messageId: number) =>
  invoke<{ view: ChatView; undo: DeleteUndo }>("delete_message", { chatId, messageId });
export const undoDelete = (chatId: number, undo: DeleteUndo) => invoke<ChatView>("undo_delete", { chatId, undo });
export const branchChat = (chatId: number, messageId: number) =>
  invoke<ChatView>("branch_chat", { chatId, messageId });

export const setChatPersona = (chatId: number, personaId: number | null) =>
  invoke<ChatView>("set_chat_persona", { chatId, personaId });
export const setChatTone = (chatId: number, tone: string, custom: string) =>
  invoke<ChatView>("set_chat_tone", { chatId, tone, custom });
export const setChatStyle = (chatId: number, style: string, custom: StyleSettings | null) =>
  invoke<ChatView>("set_chat_style", { chatId, style, custom });

export interface CharacterFields {
  name: string;
  tagline: string;
  personality: string;
  scenario: string;
  greeting: string;
  example_dialogue: string;
  voice_anchor: string;
  appearance_tags: string;
  default_tone: string;
  default_reply_style: string;
  age: number;
  is_adult: boolean;
}

export interface PersonaFields {
  name: string;
  description: string;
  appearance_tags: string;
  age: number | null;
}

export interface Staged {
  token: string;
  width: number;
  height: number;
}

export interface Crop {
  x: number;
  y: number;
  size: number;
}

export type AvatarChange = { kind: "keep" } | { kind: "remove" } | { kind: "staged"; token: string; crop: Crop };

export type TestEvent =
  | { kind: "text"; text: string }
  | { kind: "restart" }
  | { kind: "done"; text: string }
  | { kind: "failed"; error: UiError };

export interface TestLine {
  role: "user" | "character";
  content: string;
}

export const getCharacter = (characterId: number) => invoke<Character>("get_character", { characterId });
export interface WorldEntry {
  keywords: string;
  content: string;
  is_enabled: boolean;
}

/** The parts of an imported card that the creator does not show. */
export interface ImportExtras {
  extensions_json: string;
  world: WorldEntry[];
}

export type ImportNote =
  | { kind: "cut"; field: string }
  | { kind: "adult" }
  | { kind: "no_age" }
  | { kind: "dropped_prompts" }
  | { kind: "more_greetings"; count: number }
  | { kind: "world"; count: number; dropped: number }
  | { kind: "no_picture" };

export interface ImportDraft {
  fields: CharacterFields;
  extras: ImportExtras;
  notes: ImportNote[];
  picture: Staged | null;
}

export const saveCharacter = (
  characterId: number | null,
  fields: CharacterFields,
  avatar: AvatarChange,
  extras: ImportExtras | null = null,
) => invoke<Character>("save_character", { characterId, fields, avatar, extras });

/** Sends the raw bytes of a card file (PNG or JSON). Nothing is saved yet. */
export const importCard = (bytes: Uint8Array) => invoke<ImportDraft>("import_card", bytes);
export const deleteCharacter = (characterId: number) => invoke<null>("delete_character", { characterId });
export const restoreStarters = () => invoke<null>("restore_starters");
export const savePersona = (personaId: number | null, fields: PersonaFields, avatar: AvatarChange) =>
  invoke<Persona>("save_persona", { personaId, fields, avatar });
export const deletePersona = (personaId: number) => invoke<null>("delete_persona", { personaId });
export const setDefaultPersona = (personaId: number | null) => invoke<null>("set_default_persona", { personaId });
export const suggestAnchor = (name: string, personality: string) =>
  invoke<string>("suggest_anchor", { name, personality });

/** Sends the raw bytes of a picture file. The core decodes it and keeps a clean copy. */
export const stageUpload = (bytes: Uint8Array) => invoke<Staged>("stage_upload", bytes);

export function testReply(
  fields: CharacterFields,
  personaId: number | null,
  history: TestLine[],
  handler: (event: TestEvent) => void,
) {
  const ch = new Channel<TestEvent>();
  ch.onmessage = handler;
  return invoke<null>("test_reply", { fields, personaId, history, onEvent: ch });
}

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

const AVATAR_BASE = navigator.userAgent.includes("Windows") ? "http://avatar.localhost" : "avatar://localhost";

/**
 * The URL of a profile picture, served by the core. `version` changes when the
 * picture changes, so the window does not show an old copy.
 */
/** The URL of a picture that the user is cropping now. */
export function stagedUrl(token: string): string {
  return `${AVATAR_BASE}/staged/${encodeURIComponent(token)}`;
}

export function avatarUrl(kind: "character" | "persona", id: number, version: string): string {
  return `${AVATAR_BASE}/${kind}/${String(id)}?v=${encodeURIComponent(version)}`;
}
