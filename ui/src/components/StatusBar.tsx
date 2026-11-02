import { useEffect, useState } from "react";
import { modelsPage, restartEngine, type EngineStatus } from "../api";
import { strings } from "../strings";

/** `name` turns a model tag into its catalog name. */
function label(status: EngineStatus, name: (model: string) => string): string {
  switch (status.state) {
    case "starting":
      return strings.status.starting;
    case "not_installed":
      return strings.status.notInstalled;
    case "loading_model":
      return strings.status.loading(name(status.model));
    case "ready":
      return `${strings.status.ready} · ${name(status.model)}`;
    case "no_model":
      return strings.status.noModel(status.model);
    case "error":
      return status.message;
  }
}

export function StatusBar({
  status,
  onOpenModels,
  onInstallEngine,
}: {
  status: EngineStatus;
  onOpenModels: () => void;
  onInstallEngine: () => void;
}) {
  const [showDetail, setShowDetail] = useState(false);
  const [names, setNames] = useState(new Map<string, string>());
  // The catalog names, as the header and the Models screen show them.
  const ready = status.state === "ready";
  useEffect(() => {
    if (!ready) return;
    let active = true;
    modelsPage()
      .then((p) => {
        if (active) setNames(new Map(p.models.map((m) => [m.id, m.name])));
      })
      .catch(() => {
        // Without the list, the bar shows the model tag.
      });
    return () => {
      active = false;
    };
  }, [ready]);
  const name = (model: string) => names.get(model) ?? model;
  return (
    <footer className="status-bar" role="status" aria-live="polite">
      <span className={`pin-dot pin-${status.state}`} aria-hidden="true" />
      <span className="status-label">{label(status, name)}</span>
      {status.state === "not_installed" && (
        <button type="button" className="link-button" onClick={onInstallEngine}>
          {strings.status.install}
        </button>
      )}
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
              // It refuses only while a reply runs, and then the engine works.
              restartEngine().catch(() => undefined);
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
