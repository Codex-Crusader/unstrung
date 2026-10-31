import { useCallback, useEffect, useState } from "react";
import {
  getStatus,
  listPersonas,
  onStatus,
  selfTestMode,
  type EngineStatus,
  type ImportDraft,
  type Persona,
} from "./api";
import { ChatScreen } from "./components/ChatScreen";
import { CreatorScreen } from "./components/CreatorScreen";
import { HomeScreen } from "./components/HomeScreen";
import { ModelsScreen } from "./components/ModelsScreen";
import { PersonasScreen } from "./components/PersonasScreen";
import { StatusBar } from "./components/StatusBar";
import { runSelfTest } from "./selfTest";
import { strings } from "./strings";
import { Icon } from "./components/Icon";
import figure from "./assets/figure.png";

export type Screen =
  | { name: "home" }
  | { name: "chat"; characterId: number }
  | { name: "creator"; characterId: number | null; draft?: ImportDraft; draftKey?: number }
  | { name: "personas" }
  | { name: "models" };

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

export function App() {
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
          <span className="brand">
            <img className="brand-figure" src={figure} alt="" />
            {strings.appName}
          </span>
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
              key={screen.characterId}
              status={status}
              characterId={screen.characterId}
              personas={personas}
              onBack={home}
            />
          )}
          {screen.name === "creator" && (
            <CreatorScreen
              key={screen.characterId ?? `new-${String(screen.draftKey ?? 0)}`}
              characterId={screen.characterId}
              draft={screen.draft ?? null}
              ready={ready}
              onSaved={(c) => {
                setScreen({ name: "chat", characterId: c.id });
              }}
              onDeleted={home}
              onCancel={home}
            />
          )}
          {screen.name === "personas" && <PersonasScreen onChanged={loadPersonas} />}
          {screen.name === "models" && (
            <ModelsScreen engine={`${status.state}:${"model" in status ? status.model : ""}`} />
          )}
        </main>
      </div>
      <StatusBar
        status={status}
        onOpenModels={() => {
          setScreen({ name: "models" });
        }}
      />
    </div>
  );
}
