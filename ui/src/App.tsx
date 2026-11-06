import { useCallback, useEffect, useState } from "react";
import {
  getStatus,
  listPersonas,
  onStatus,
  selfTestMode,
  setupState,
  startMode,
  startOk,
  type EngineStatus,
  type ImportDraft,
  type Staged,
  type Persona,
  type StartView,
  getUiSettings,
} from "./api";
import { ChatScreen } from "./components/ChatScreen";
import { CreatorScreen } from "./components/CreatorScreen";
import { HomeScreen } from "./components/HomeScreen";
import { ModelsScreen } from "./components/ModelsScreen";
import { PersonasScreen } from "./components/PersonasScreen";
import { StatusBar } from "./components/StatusBar";
import { runSelfTest, runWelcomeSelfTest } from "./selfTest";
import { strings } from "./strings";
import { Icon } from "./components/Icon";
import { SafeStart } from "./components/SafeStart";
import { Welcome } from "./components/Welcome";
import { PicturesScreen } from "./components/PicturesScreen";
import { ChatsScreen } from "./components/ChatsScreen";
import { SettingsScreen } from "./components/SettingsScreen";
import { UpdateBar } from "./components/Updates";
import { applyAppearance } from "./appearance";
import { Splash } from "./components/Loading";
import { Brand } from "./components/Brand";

export type Screen =
  | { name: "home" }
  | { name: "chat"; characterId: number; chatId?: number }
  | { name: "creator"; characterId: number | null; draft?: ImportDraft; draftKey?: number; picture?: Staged }
  | { name: "chats" }
  | { name: "personas" }
  | { name: "models" }
  | { name: "pictures" }
  | { name: "settings" }
  | { name: "engine" };

/** Closes open message menus on a click outside them, or on Escape. */
function useMenuDismiss() {
  useEffect(() => {
    const closeAll = (keep: Node | null) => {
      document.querySelectorAll("details.menu[open]").forEach((menu) => {
        if (!keep || !menu.contains(keep)) menu.removeAttribute("open");
      });
    };
    const onPointer = (e: PointerEvent) => {
      closeAll(e.target instanceof Node ? e.target : null);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeAll(null);
    };
    document.addEventListener("pointerdown", onPointer);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("pointerdown", onPointer);
      document.removeEventListener("keydown", onKey);
    };
  }, []);
}

/** Asks for the start mode first. Safe start shows only its own screen. */
export function App() {
  const [start, setStart] = useState<StartView | null>(null);
  useEffect(() => {
    startMode()
      .then(setStart)
      .catch(() => {
        setStart({ safe: false, reason: null, error: "" });
      });
  }, []);
  if (!start) return <Splash />;
  return start.safe ? <SafeStart start={start} /> : <FirstRunGate />;
}

/** A new install shows the welcome flow before the app. */
function FirstRunGate() {
  const [firstRun, setFirstRun] = useState<boolean | null>(null);
  useEffect(() => {
    // The window loaded, so the next start is a normal one. Also in the
    // welcome flow, which tells the user they can close it during downloads.
    void startOk().catch(() => {
      // Only the start count stays. The next start still opens normally once.
    });
    getUiSettings()
      .then(applyAppearance)
      .catch(() => undefined);
    setupState()
      .then((s) => {
        setFirstRun(s.first_run);
        if (s.first_run)
          void selfTestMode().then((on) => {
            if (on) void runWelcomeSelfTest();
          });
      })
      .catch(() => {
        setFirstRun(false);
      });
  }, []);
  if (firstRun === null) return <Splash />;
  if (firstRun)
    return (
      <Welcome
        mode="full"
        onDone={() => {
          setFirstRun(false);
        }}
      />
    );
  return <MainApp />;
}

function MainApp() {
  const [status, setStatus] = useState<EngineStatus>({ state: "starting" });
  const [screen, setScreen] = useState<Screen>({ name: "home" });
  const [personas, setPersonas] = useState<Persona[]>([]);
  useMenuDismiss();

  useEffect(() => {
    let active = true;
    const unlisten = onStatus((next) => {
      if (active) setStatus(next);
    });
    // Events only report changes, so read the state once at the start.
    void getStatus().then((current) => {
      if (active) setStatus(current);
    });
    void selfTestMode().then((on) => {
      if (on)
        void runSelfTest((next) => {
          setScreen(next);
        });
    });
    return () => {
      active = false;
      void unlisten.then((stop) => {
        stop();
      });
    };
  }, []);

  const loadPersonas = useCallback(() => {
    listPersonas()
      .then(setPersonas)
      .catch(() => {
        setPersonas([]);
      });
  }, []);
  useEffect(loadPersonas, [loadPersonas]);

  const home = () => {
    setScreen({ name: "home" });
  };
  const ready = status.state === "ready";

  return (
    <div className="app">
      <div className="app-body">
        <nav className="sidebar" aria-label={strings.nav.main}>
          <Brand />
          <button
            type="button"
            className={screen.name === "home" ? "nav-item nav-current" : "nav-item"}
            onClick={home}
          >
            <Icon name="home" />
            {strings.nav.home}
          </button>
          <button
            type="button"
            className={screen.name === "chats" ? "nav-item nav-current" : "nav-item"}
            onClick={() => {
              setScreen({ name: "chats" });
            }}
          >
            <Icon name="chats" />
            {strings.nav.chats}
          </button>
          <button
            type="button"
            className={screen.name === "personas" ? "nav-item nav-current" : "nav-item"}
            onClick={() => {
              setScreen({ name: "personas" });
            }}
          >
            <Icon name="personas" />
            {strings.nav.personas}
          </button>
          <button
            type="button"
            className={screen.name === "models" ? "nav-item nav-current" : "nav-item"}
            onClick={() => {
              setScreen({ name: "models" });
            }}
          >
            <Icon name="models" />
            {strings.nav.models}
          </button>
          <button
            type="button"
            className={screen.name === "pictures" ? "nav-item nav-current" : "nav-item"}
            onClick={() => {
              setScreen({ name: "pictures" });
            }}
          >
            <Icon name="picture" />
            {strings.nav.pictures}
          </button>
          <button
            type="button"
            className={screen.name === "settings" ? "nav-item nav-current" : "nav-item"}
            onClick={() => {
              setScreen({ name: "settings" });
            }}
          >
            <Icon name="sliders" />
            {strings.nav.settings}
          </button>
        </nav>
        <main className="app-main">
          {screen.name === "home" && (
            <HomeScreen
              onOpen={(c) => {
                setScreen({ name: "chat", characterId: c.id });
              }}
              onCreate={() => {
                setScreen({ name: "creator", characterId: null });
              }}
              onEdit={(c) => {
                setScreen({ name: "creator", characterId: c.id });
              }}
              onImport={(draft) => {
                setScreen({ name: "creator", characterId: null, draft, draftKey: Date.now() });
              }}
            />
          )}
          {screen.name === "chat" && (
            <ChatScreen
              key={`${String(screen.characterId)}:${String(screen.chatId ?? "")}`}
              status={status}
              characterId={screen.characterId}
              chatId={screen.chatId}
              personas={personas}
              onBack={home}
              onUseAsAvatar={(characterId, staged) => {
                setScreen({ name: "creator", characterId, picture: staged });
              }}
            />
          )}
          {screen.name === "creator" && (
            <CreatorScreen
              key={screen.characterId ?? `new-${String(screen.draftKey ?? 0)}`}
              characterId={screen.characterId}
              draft={screen.draft ?? null}
              picture={screen.picture ?? null}
              ready={ready}
              onSaved={(c) => {
                setScreen({ name: "chat", characterId: c.id });
              }}
              onDeleted={home}
              onCancel={home}
            />
          )}
          {screen.name === "chats" && (
            <ChatsScreen
              onOpenChat={(characterId, chatId) => {
                setScreen({ name: "chat", characterId, chatId });
              }}
            />
          )}
          {screen.name === "personas" && <PersonasScreen onChanged={loadPersonas} />}
          {screen.name === "models" && (
            <ModelsScreen
              engine={`${status.state}:${"model" in status ? status.model : ""}`}
              onOpenPictures={() => {
                setScreen({ name: "pictures" });
              }}
            />
          )}
          {screen.name === "settings" && <SettingsScreen />}
          {screen.name === "pictures" && (
            <PicturesScreen
              onOpenChat={(characterId, chatId) => {
                setScreen({ name: "chat", characterId, chatId });
              }}
              onUseAsAvatar={(p, staged) => {
                setScreen({ name: "creator", characterId: p.character_id, picture: staged });
              }}
              onOpenModels={() => {
                setScreen({ name: "models" });
              }}
            />
          )}
          {screen.name === "engine" && <Welcome mode="engine" onDone={home} />}
        </main>
      </div>
      <UpdateBar />
      <StatusBar
        status={status}
        onOpenModels={() => {
          setScreen({ name: "models" });
        }}
        onInstallEngine={() => {
          setScreen({ name: "engine" });
        }}
      />
    </div>
  );
}
