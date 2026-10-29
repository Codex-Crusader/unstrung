import { useEffect, useState } from "react";
import { getStatus, onStatus, selfTestMode, type EngineStatus } from "./api";
import { ChatScreen } from "./components/ChatScreen";
import { StatusBar } from "./components/StatusBar";
import { runSelfTest } from "./selfTest";

export function App() {
  const [status, setStatus] = useState<EngineStatus>({ state: "starting" });

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
      if (on) void runSelfTest();
    });
    return () => {
      active = false;
      void unlisten.then((stop) => {
        stop();
      });
    };
  }, []);

  return (
    <div className="app">
      <main className="app-main">
        <ChatScreen status={status} />
      </main>
      <StatusBar status={status} />
    </div>
  );
}
