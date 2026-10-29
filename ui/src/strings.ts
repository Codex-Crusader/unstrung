// All UI text in one place, so translations can come later.

export const strings = {
  appName: "Unstrung",
  status: {
    starting: "Waking up your characters...",
    loading: (model: string) => `Loading ${model}...`,
    ready: "Ready",
    retry: "Retry",
    details: "Details",
    hideDetails: "Hide details",
  },
  chat: {
    placeholder: (name: string) => `Message ${name}`,
    notReady: (name: string) => `Getting ${name} ready...`,
    send: "Send",
    stop: "Stop",
    tryAgain: "Try again",
    thinking: (name: string) => `${name} is typing`,
    messages: "Messages",
    you: "You",
    loadFailed: "The chat could not open.",
  },
} as const;
