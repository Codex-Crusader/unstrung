import { useState } from "react";
import { restartEngine, type EngineStatus } from "../api";
import { strings } from "../strings";

function label(status: EngineStatus): string {
  switch (status.state) {
    case "starting":
      return strings.status.starting;
    case "loading_model":
      return strings.status.loading(status.model);
    case "ready":
      return `${strings.status.ready} · ${status.model}`;
    case "no_model":
      return strings.status.noModel(status.model);
    case "error":
      return status.message;
  }
}

export function StatusBar({ status, onOpenModels }: { status: EngineStatus; onOpenModels: () => void }) {
  const [showDetail, setShowDetail] = useState(false);
  return (
    <footer className="status-bar" role="status" aria-live="polite">
      <span className={`pin-dot pin-${status.state}`} aria-hidden="true" />
      <span className="status-label">{label(status)}</span>
      {status.state === "no_model" && (
        <button type="button" className="link-button" onClick={onOpenModels}>
          {strings.status.openModels}
        </button>
      )}
      {status.state === "error" && (
        <>
          <button
            type="button"
            className="link-button"
            onClick={() => {
              setShowDetail(false);
              void restartEngine();
            }}
          >
            {strings.status.retry}
          </button>
          <button
            type="button"
            className="link-button"
            onClick={() => {
              setShowDetail((v) => !v);
            }}
          >
            {showDetail ? strings.status.hideDetails : strings.status.details}
          </button>
          {showDetail && <span className="status-detail">{status.detail}</span>}
        </>
      )}
    </footer>
  );
}
