import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { CrashGuard } from "./components/CrashScreen";
import "./styles.css";

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <CrashGuard>
        <App />
      </CrashGuard>
    </StrictMode>,
  );
}
