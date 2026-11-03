// Typed wrappers for the core commands. The UI has no other way to reach
// the disk, the network or the engines.

import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type EngineStatus =
  | { state: "starting" }
  | { state: "not_installed" }
  | { state: "loading_model"; model: string }
  | { state: "ready"; model: string }
  | { state: "no_model"; model: string }
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
  outfit: string;
  art_style: string;
  default_tone: string;
  default_reply_style: string;
  default_reply_custom: string;
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
  outfit: string;
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
  /** "Refresh context": the prompt starts near this message. */
  context_start_id: number | null;
  /** The chat uses the character's kept memories. */
  use_character_memory: boolean;
  created_at: number;
  updated_at: number;
}

export type MemoryMode = "off" | "ask" | "always";

/** Facts a character keeps for new chats, and whether new chats use them. */
export interface Kept {
  mode: MemoryMode;
  memories: { id: number; content: string; created_at: number }[];
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
  /** The model that writes the next reply. */
  model: string;
  /** The chat's own model, when it was removed. The default writes instead. */
  model_removed: string | null;
  /** Adult content is on, the character is 18+ and the persona has no age: ask it once. */
  ask_persona_age: boolean;
}

/** One thing a chat remembers. See `db::Memory` in the core. */
export interface Memory {
  id: number;
  chat_id: number;
  kind: "goal" | "summary" | "fact";
  content: string;
  source_message_id: number;
  origin: "auto" | "user";
  is_pinned: boolean;
  is_hidden: boolean;
  created_at: number;
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
  /** "antivirus": another program keeps locking files. */
  tip?: "antivirus";
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

export type Tier = "basic" | "standard" | "high";

export interface Hardware {
  gpu: { name: string; vendor: string; vram_bytes: number; driver: string } | null;
  ram_bytes: number;
  disk_free_bytes: number;
  avx2: boolean;
  tier: Tier;
  below_minimum: boolean;
}

export interface ModelView {
  id: string;
  name: string;
  description: string;
  size_bytes: number;
  installed: boolean;
  in_catalog: boolean;
  recommended: boolean;
  fits: boolean;
  min_tier: Tier | null;
  license_name: string;
  license_url: string;
  style_following: "good" | "fair" | "weak";
  can_think: boolean;
  is_default: boolean;
  changed_upstream: boolean;
}

/** `null` means "Automatic": the catalog value. */
export interface Advanced {
  context: number | null;
  temperature: number | null;
  min_p: number | null;
  repeat_penalty: number | null;
  think: boolean | null;
}

export interface Download {
  model: string;
  state: "downloading" | "importing" | "checking" | "done" | "stopped" | "failed";
  completed: number;
  total: number;
  bytes_per_second: number;
  error: string;
}

export interface ModelsPage {
  hardware: Hardware;
  models: ModelView[];
  default_model: string;
  advanced: Advanced;
  shared_folder: boolean;
  download: Download | null;
  hardware_changed: boolean;
}

export const modelsPage = () => invoke<ModelsPage>("models_page");
export const recheckHardware = () => invoke<ModelsPage>("recheck_hardware");
export const downloadModel = (model: string) => invoke<null>("download_model", { model });
export const stopDownload = () => invoke<null>("stop_download");
export const removeModel = (model: string) => invoke<ModelsPage>("remove_model", { model });
export const setDefaultModel = (model: string) => invoke<ModelsPage>("set_default_model", { model });
export const setAdvanced = (advanced: Advanced) => invoke<ModelsPage>("set_advanced", { advanced });
export const setChatModel = (chatId: number, model: string | null) =>
  invoke<ChatView>("set_chat_model", { chatId, model });
export const setCharacterModel = (characterId: number, model: string | null) =>
  invoke<null>("set_character_model", { characterId, model });

export function onDownload(handler: (download: Download | null) => void): Promise<UnlistenFn> {
  return listen<Download | null>("model-download", (event) => {
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
/** `useMemory` answers the question of the "ask" memory setting. */
export const newChat = (characterId: number, personaId: number | null, useMemory = false) =>
  invoke<ChatView>("new_chat", { characterId, personaId, useMemory });
/** The next replies see the summary, the memories and only the last lines. */
export const refreshContext = (chatId: number) => invoke<ChatView>("refresh_context", { chatId });
/** A new chat that goes on from this one, with its notes. */
export const nextChapter = (chatId: number) => invoke<ChatView>("next_chapter", { chatId });
/** The story so far, written again from the whole chat. Nothing is saved yet. */
export interface RebuiltSummary {
  summary: string;
  through_id: number;
}
export const rebuildSummary = (chatId: number) => invoke<RebuiltSummary>("rebuild_summary", { chatId });
export const saveSummary = (chatId: number, throughId: number, text: string) =>
  invoke<Memory[]>("save_summary", { chatId, throughId, text });
export interface Exported {
  bytes: number;
  avatars: number;
  pictures: number;
}
/** Opens a save picker. `null` when the user closes it. */
export const exportBackup = () => invoke<Exported | null>("export_backup");
export interface BackupInfo {
  id: string;
  created_ms: number;
  app_version: string;
  characters: number;
  personas: number;
  chats: number;
  pictures: number;
}
export interface Added {
  characters: number;
  personas: number;
  skipped: number;
}
/** Opens a file picker, then checks and stages the backup. `null` when the user closes it. */
export const openBackup = () => invoke<BackupInfo | null>("open_backup");
export const addFromBackup = (id: string) => invoke<Added>("add_from_backup", { id });
/** Restarts the app, which then swaps all data for the backup. */
export const replaceFromBackup = (id: string) => invoke<null>("replace_from_backup", { id });
export type UsageKind =
  | "chat_models"
  | "picture_models"
  | "gallery"
  | "chats"
  | "set_aside"
  | "engines"
  | "logs"
  | "temp";
export interface Usage {
  parts: { kind: UsageKind; bytes: number }[];
  disks: { folders: ("app" | "library" | "models")[]; free: number; low: boolean }[];
  library: string;
  models: string;
}
/** Moves the pictures or the models. `onProgress` gets bytes copied and bytes to copy. Stop: `stopReply`. */
export function moveFolder(purpose: FolderPurpose, path: string, onProgress: (done: number, total: number) => void) {
  const ch = new Channel<[number, number]>();
  ch.onmessage = ([done, total]) => {
    onProgress(done, total);
  };
  // The old folders that could not be deleted.
  return invoke<string[]>("move_folder", { purpose, path, onProgress: ch });
}
export interface Candidate {
  id: "temp" | "deleted_chats" | "set_aside";
  bytes: number;
  items: string[];
  /** Deleted chats of 18+ characters that go too, not named while adult content is off. */
  hidden: number;
  checked: boolean;
}
export const freeUpCandidates = () => invoke<Candidate[]>("free_up_candidates");
/** Removes the picked lines at once. Ask the user first. */
export const freeUpSpace = (ids: string[]) => invoke<number>("free_up_space", { ids });
export const storageUsage = () => invoke<Usage>("storage_usage");
export const discardBackup = (id: string) => invoke<null>("discard_backup", { id });
export const undoRefresh = (chatId: number) => invoke<ChatView>("undo_refresh", { chatId });
export const keptMemories = (characterId: number) => invoke<Kept>("kept_memories", { characterId });
export const keepMemory = (chatId: number, memoryId: number) => invoke<Kept>("keep_memory", { chatId, memoryId });
export const forgetKeptMemory = (characterId: number, memoryId: number) =>
  invoke<Kept>("forget_kept_memory", { characterId, memoryId });
export const setMemoryMode = (characterId: number, mode: MemoryMode) =>
  invoke<Kept>("set_memory_mode", { characterId, mode });
export const setChatUseMemory = (chatId: number, on: boolean) => invoke<null>("set_chat_use_memory", { chatId, on });
export const listChats = (characterId: number) => invoke<ChatSummary[]>("list_chats", { characterId });
export const deleteChat = (chatId: number) => invoke<null>("delete_chat", { chatId });
/** Deleted chats stay 7 days, then go for good at the next start. */
export const deletedChats = () => invoke<DeletedChat[]>("deleted_chats");
export const restoreChat = (chatId: number) => invoke<null>("restore_chat", { chatId });

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

export const chatMemories = (chatId: number) => invoke<Memory[]>("chat_memories", { chatId });

/** Calls `handler` with the chat id each time a memory pass changes that chat. */
export function onMemoryChanged(handler: (chatId: number) => void): Promise<UnlistenFn> {
  return listen<number>("chat-memory", (event) => {
    handler(event.payload);
  });
}
export const remember = (chatId: number, messageId: number, text: string) =>
  invoke<Memory[]>("remember", { chatId, messageId, text });
export const editMemory = (chatId: number, memoryId: number, text: string) =>
  invoke<Memory[]>("edit_memory", { chatId, memoryId, text });
export const setMemoryFlags = (chatId: number, memoryId: number, pinned: boolean, hidden: boolean) =>
  invoke<Memory[]>("set_memory_flags", { chatId, memoryId, pinned, hidden });

export interface CharacterFields {
  name: string;
  tagline: string;
  personality: string;
  scenario: string;
  greeting: string;
  example_dialogue: string;
  voice_anchor: string;
  appearance_tags: string;
  outfit: string;
  art_style: string;
  default_tone: string;
  default_reply_style: string;
  default_reply_custom: string;
  age: number;
  is_adult: boolean;
}

export interface PersonaFields {
  name: string;
  description: string;
  appearance_tags: string;
  outfit: string;
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
export const getAdultContent = () => invoke<boolean>("get_adult_content");
/** Ask "Are you 18 or older?" before `true`. */
export const setAdultContent = (on: boolean) => invoke<null>("set_adult_content", { on });
export const savePersona = (personaId: number | null, fields: PersonaFields, avatar: AvatarChange) =>
  invoke<Persona>("save_persona", { personaId, fields, avatar });
export const deletePersona = (personaId: number) => invoke<null>("delete_persona", { personaId });
export const setDefaultPersona = (personaId: number | null) => invoke<null>("set_default_persona", { personaId });
export const suggestAnchor = (name: string, personality: string) =>
  invoke<string>("suggest_anchor", { name, personality });
/** Looks and a usual outfit from the card's text. Nothing is saved. */
export const suggestLooks = (name: string, card: string) =>
  invoke<{ looks: string; outfit: string }>("suggest_looks", { name, card });

/** Reads Looks from a profile picture: `token` for an unsaved upload, else the saved `avatar`. */
export const suggestLooksFromPicture = (name: string, picture: { token?: string; avatar?: string }) =>
  invoke<{ looks: string; outfit: string }>("suggest_looks_from_picture", {
    name,
    token: picture.token ?? null,
    avatar: picture.avatar ?? null,
  });

/** True when the chosen chat model can read pictures. */
export const canReadPictures = () => invoke<boolean>("can_read_pictures");

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

export type SafeReason = "failed_starts" | "asked" | "data_did_not_open" | "import_failed";

export interface StartView {
  safe: boolean;
  reason: SafeReason | null;
  /** Why the chat data did not open. */
  error: string;
}

export const startMode = () => invoke<StartView>("start_mode");
/** The window loaded, so this start counts as good. */
export const startOk = () => invoke<null>("start_ok");
export const startNormally = () => invoke<null>("start_normally");
export const diagnosticReport = () => invoke<string>("diagnostic_report");

export type RepairArea = "engine" | "data" | "temp_files";
export type RepairState = "ok" | "can_fix" | "fixed" | "needs_action";

export interface Finding {
  area: RepairArea;
  state: RepairState;
  /** Bytes of temporary files, found or freed. */
  bytes: number;
  detail: string;
}

export type EngineStep = "downloading" | "unpacking" | "checking" | "done" | "stopped" | "failed";

export interface EngineProgress {
  step: EngineStep;
  completed: number;
  total: number;
  bytes_per_second: number;
  error: string;
}

export interface SetupState {
  first_run: boolean;
  engine_installed: string | null;
  engine_version: string;
  engine_download_bytes: number;
  engine_disk_bytes: number;
  engine_progress: EngineProgress | null;
  choice: string | null;
  app_folder: string;
  library: string;
  models: string;
  default_library: string;
  default_models: string;
}

export type FolderPurpose = "library" | "models";
export type FolderIssue =
  | "system_folder"
  | "not_absolute"
  | "not_writable"
  | "synced"
  | "network_drive"
  | "removable_drive"
  | "unusual_characters"
  | "not_enough_space";

export interface FolderCheck {
  folder: string;
  issues: FolderIssue[];
  blocked: boolean;
  free_bytes: number;
  need_bytes: number;
  existing_library: boolean;
}

export interface RecentChat {
  chat_id: number;
  character_id: number;
  character_name: string;
  avatar_path: string | null;
  preview: string;
  updated_at: number;
}

export interface DeletedChat {
  chat_id: number;
  character_id: number;
  character_name: string;
  avatar_path: string | null;
  preview: string;
  deleted_at: number;
}

/** Opens the file picker and starts the import. `null` when the user cancels. */
export const importModelFile = () => invoke<string | null>("import_model_file");

/** A download or an import is running. */
export const downloadRunning = (d: Download | null) =>
  d !== null && (d.state === "downloading" || d.state === "importing" || d.state === "checking");

export const openWindowsSecurity = () => invoke<null>("open_windows_security");

export const recentChats = () => invoke<RecentChat[]>("recent_chats");

export const setupState = () => invoke<SetupState>("setup_state");
export const pickFolder = (start: string | null) => invoke<string | null>("pick_folder", { start });
export const checkFolder = (path: string, purpose: FolderPurpose, downloadBytes: number) =>
  invoke<FolderCheck>("check_folder", { path, purpose, downloadBytes });
/** `null` keeps the default folder. First run only. */
export const setLocation = (library: string | null, models: string | null, downloadBytes: number) =>
  invoke<SetupState>("set_location", { library, models, downloadBytes });
export const engineInstall = () => invoke<null>("engine_install");
export const engineInstallCancel = () => invoke<null>("engine_install_cancel");
export const chooseSetup = (model: string) => invoke<null>("choose_setup", { model });
export const finishFirstRun = () => invoke<null>("finish_first_run");
export const getNotify = () => invoke<boolean>("get_notify");
export const setNotify = (on: boolean) => invoke<null>("set_notify", { on });

/** A newer version of Unstrung on GitHub Releases. */
export interface UpdateInfo {
  version: string;
  notes: string;
}
export const checkUpdate = () => invoke<UpdateInfo | null>("check_update");
/** Downloads and installs. On success the app closes and the new version opens. */
export const installUpdate = () => invoke<null>("install_update");
export const getUpdateCheck = () => invoke<boolean>("get_update_check");
export const setUpdateCheck = (on: boolean) => invoke<null>("set_update_check", { on });

export function onUpdateAvailable(handler: (update: UpdateInfo) => void): Promise<UnlistenFn> {
  return listen<UpdateInfo>("update-available", (event) => {
    handler(event.payload);
  });
}

/** Bytes downloaded, and the size when the server says it. */
export function onUpdateProgress(handler: (progress: [number, number | null]) => void): Promise<UnlistenFn> {
  return listen<[number, number | null]>("update-progress", (event) => {
    handler(event.payload);
  });
}

export function onEngineSetup(handler: (progress: EngineProgress | null) => void): Promise<UnlistenFn> {
  return listen<EngineProgress | null>("engine-setup", (event) => {
    handler(event.payload);
  });
}

export function onEngineSetupError(handler: (error: UiError) => void): Promise<UnlistenFn> {
  return listen<UiError>("engine-setup-error", (event) => {
    handler(event.payload);
  });
}

/** Quick repair, step 1: looks and changes nothing. Safe start only. */
export const repairCheck = (full = false) => invoke<Finding[]>("repair_check", { full });
/** Quick repair, step 2: checks again and fixes. Safe start only. */
export const repairFix = (full = false) => invoke<Finding[]>("repair_fix", { full });

/** A copy of the chats file: one a day, and one before each update. */
export interface Snapshot {
  name: string;
  kind: "daily" | "update";
  created_ms: number;
  bytes: number;
}

/** The copies of the chats file, newest first. */
export const listCopies = () => invoke<Snapshot[]>("list_copies");
/** Puts a copy back as the chats file. Safe start only. */
export const restoreCopy = (name: string) => invoke<null>("restore_copy", { name });

/** Theme, text size and motion: `ui_theme`, `ui_text_size`, `ui_motion`. */
export const getUiSettings = () => invoke<Record<string, string>>("get_ui_settings");
export const setUiSetting = (key: string, value: string) => invoke<null>("set_ui_setting", { key, value });
/** Restarts the app in safe start. */
export const restartSafe = () => invoke<null>("restart_safe");

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

/**
 * A gallery picture. The database can give a deleted picture's id to a new
 * one, so the time it was made keeps the window from showing the old file.
 */
export function pictureUrl(picture: { id: number; created_at: number }): string {
  return `${AVATAR_BASE}/picture/${String(picture.id)}?v=${String(picture.created_at)}`;
}

export interface Picture {
  id: number;
  chat_id: number;
  message_id: number | null;
  prompt: string;
  seed: number;
  image_model: string;
  width: number;
  height: number;
  /** Made under adult rules. The gallery blurs it until it is opened. */
  adult: boolean;
  created_at: number;
}

export interface GalleryPicture extends Picture {
  character_id: number;
  character_name: string;
}

export interface PictureModel {
  id: string;
  name: string;
  description: string;
  size_bytes: number;
  missing_bytes: number;
  installed: boolean;
  fits: boolean;
  recommended: boolean;
  selected: boolean;
  mature_ok: boolean;
  license_name: string;
  license_url: string;
  tested: boolean;
}

export interface PictureDownload {
  model: string;
  part: "engine" | "model";
  state: "running" | "done" | "stopped" | "failed";
  completed: number;
  total: number;
  bytes_per_second: number;
  error: string;
}

export interface PicturesPage {
  tier: Tier;
  cpu_only: boolean;
  engine_installed: boolean;
  engine_download_bytes: number;
  models: PictureModel[];
  selected: string | null;
  download: PictureDownload | null;
}

export interface Painting {
  chat_id: number;
  message_id: number | null;
  phase: "scene" | "loading" | "painting" | "finishing";
  step: number;
  total: number;
}

export const picturesPage = () => invoke<PicturesPage>("pictures_page");
export const downloadPictureModel = (id: string) => invoke<null>("download_picture_model", { id });
export const stopPictureDownload = () => invoke<null>("stop_picture_download");
export const selectPictureModel = (id: string) => invoke<null>("select_picture_model", { id });
export const removePictureModel = (id: string) => invoke<null>("remove_picture_model", { id });
export const paintPicture = (chatId: number, messageId: number | null, wish: string) =>
  invoke<Picture | null>("paint_picture", { chatId, messageId, wish });
export const repaintPicture = (id: number) => invoke<Picture | null>("repaint_picture", { id });
export const stagePicture = (id: number) => invoke<Staged>("stage_picture", { id });
export const exportPicture = (id: number, name: string) => invoke<boolean>("export_picture", { id, name });
export const chatPictures = (chatId: number) => invoke<Picture[]>("chat_pictures", { chatId });
export const gallery = () => invoke<GalleryPicture[]>("gallery");
export const deletePicture = (id: number) => invoke<null>("delete_picture", { id });

export function onPictureDownload(handler: (download: PictureDownload | null) => void): Promise<UnlistenFn> {
  return listen<PictureDownload | null>("picture-download", (event) => {
    handler(event.payload);
  });
}

export function onPainting(handler: (painting: Painting | null) => void): Promise<UnlistenFn> {
  return listen<Painting | null>("picture-painting", (event) => {
    handler(event.payload);
  });
}

export function avatarUrl(kind: "character" | "persona", id: number, version: string): string {
  return `${AVATAR_BASE}/${kind}/${String(id)}?v=${encodeURIComponent(version)}`;
}
