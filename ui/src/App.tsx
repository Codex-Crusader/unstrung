import { useEffect, useState } from "react";
import { getStatus, listPersonas, onStatus, selfTestMode, type EngineStatus, type Persona } from "./api";
import { ChatScreen } from "./components/ChatScreen";
import { HomeScreen } from "./components/HomeScreen";
import { StatusBar } from "./components/StatusBar";
import { runSelfTest } from "./selfTest";
import { strings } from "./strings";

export type Screen = { name: "home" } | { name: "chat"; characterId: number };

export function App() {
  const [status, setStatus] = useState<EngineStatus>({ state: "starting" });
  const [screen, setScreen] = useState<Screen>({ name: "home" });
  const [personas, setPersonas] = useState<Persona[]>([]);

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
        void runSelfTest((characterId) => {
          setScreen({ name: "chat", characterId });
        });
    });
    return () => {
      active = false;
      void unlisten.then((stop) => {
        stop();
      });
    };
  }, []);

  useEffect(() => {
    void listPersonas().then(setPersonas);
  }, [screen]);

  const home = () => {
    setScreen({ name: "home" });
  };

  return (
    <div className="app">
      <div className="app-body">
        <nav className="sidebar" aria-label={strings.nav.main}>
          <span className="brand">{strings.appName}</span>
          <button
            type="button"
            className={screen.name === "home" ? "nav-item nav-current" : "nav-item"}
            onClick={home}
          >
            {strings.nav.home}
          </button>
        </nav>
        <main className="app-main">
          {screen.name === "home" && (
            <HomeScreen
              onOpen={(c) => {
                setScreen({ name: "chat", characterId: c.id });
              }}
              onCreate={home}
              onEdit={home}
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
        </main>
      </div>
      <StatusBar status={status} />
    </div>
  );
}
