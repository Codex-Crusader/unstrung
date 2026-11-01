import { useEffect, useMemo, useRef, useState } from "react";
import {
  checkFolder,
  chooseSetup,
  downloadModel,
  engineInstall,
  engineInstallCancel,
  restartEngine,
  finishFirstRun,
  getNotify,
  getStatus,
  listPersonas,
  modelsPage,
  onDownload,
  onEngineSetup,
  onEngineSetupError,
  onStatus,
  pickFolder,
  savePersona,
  setDefaultModel,
  setDefaultPersona,
  setLocation,
  setNotify,
  setupState,
  stopDownload,
  toUiError,
  type Download,
  type EngineProgress,
  type EngineStatus,
  type FolderCheck,
  type ModelView,
  type ModelsPage,
  type Persona,
  type SetupState,
  type UiError,
} from "../api";
import { strings } from "../strings";
import { Icon } from "./Icon";
import { ErrorNote } from "./ErrorNote";
import { Avatar } from "./Avatar";
import figure from "../assets/figure.png";

const GB = 1024 * 1024 * 1024;
const STEPS = ["welcome", "hardware", "setup", "storage", "download", "persona"] as const;
type Step = (typeof STEPS)[number];

function gb(bytes: number): string {
  return `${(bytes / GB).toFixed(1)} GB`;
}

/**
 * The first run: welcome, hardware check, setup, storage, download, persona.
 * `engine` mode shows only the engine download, for an install that has no
 * engine (for example after a repair).
 */
export function Welcome({ mode, onDone }: { mode: "full" | "engine"; onDone: () => void }) {
  const [step, setStep] = useState<Step | null>(null);
  const [setup, setSetup] = useState<SetupState | null>(null);
  const [page, setPage] = useState<ModelsPage | null>(null);
  const [choice, setChoice] = useState<string | null>(null);
  const [error, setError] = useState<UiError | null>(null);

  useEffect(() => {
    Promise.all([setupState(), modelsPage()])
      .then(([state, models]) => {
        setSetup(state);
        setPage(models);
        if (mode === "engine") {
          setStep("download");
          return;
        }
        const picked = state.choice ?? null;
        setChoice(picked);
        if (picked === null) {
          setStep("welcome");
          return;
        }
        // Closed during the first run: go on from the download.
        const installed = models.models.some((m) => m.id === picked && m.installed);
        setStep(state.engine_installed !== null && installed ? "persona" : "download");
      })
      .catch((e: unknown) => {
        setError(toUiError(e));
      });
  }, [mode]);

  if (error) {
    return (
      <div className="welcome">
        <ErrorNote error={error} />
      </div>
    );
  }
  if (!setup || !page || !step) return null;
  const go = (next: Step) => {
    setStep(next);
  };
  const index = STEPS.indexOf(step);
  const engineBytes = setup.engine_installed === null ? setup.engine_download_bytes : 0;
  const chosen = page.models.find((m) => m.id === choice) ?? null;
  const modelBytes = chosen && !chosen.installed ? chosen.size_bytes : 0;

  return (
    <div className="welcome">
      <div className="welcome-card">
        <div className="brand welcome-brand">
          <img src={figure} alt="" className="brand-figure" />
          <span>Unstrung</span>
        </div>
        {mode === "full" && step !== "welcome" && (
          <p className="field-hint welcome-step">{strings.welcome.stepOf(index, STEPS.length - 1)}</p>
        )}
        {step === "welcome" && <Hello onNext={() => { go("hardware"); }} />}
        {step === "hardware" && <Hardware page={page} onNext={() => { go("setup"); }} />}
        {step === "setup" && (
          <Setup
            page={page}
            engineBytes={engineBytes}
            choice={choice}
            onPick={setChoice}
            onBack={() => { go("hardware"); }}
            onNext={(model) => {
              void chooseSetup(model).then(() => {
                go("storage");
              }, (e: unknown) => {
                setError(toUiError(e));
              });
            }}
          />
        )}
        {step === "storage" && (
          <Storage
            setup={setup}
            downloadBytes={engineBytes + modelBytes}
            onBack={() => { go("setup"); }}
            onNext={(next) => {
              setSetup(next);
              go("download");
            }}
          />
        )}
        {step === "download" && (
          <Downloads
            mode={mode}
            setup={setup}
            model={mode === "full" ? chosen : null}
            onNext={() => {
              if (mode === "engine") onDone();
              else go("persona");
            }}
          />
        )}
        {step === "persona" && (
          <PersonaStep
            onDone={() => {
              void finishFirstRun().then(onDone, (e: unknown) => {
                setError(toUiError(e));
              });
            }}
          />
        )}
      </div>
    </div>
  );
}

function Hello({ onNext }: { onNext: () => void }) {
  return (
    <section className="welcome-section welcome-hello">
      <h1>Unstrung</h1>
      <p className="welcome-tagline">{strings.welcome.tagline}</p>
      <div className="safe-actions">
        <button type="button" className="button button-big" onClick={onNext} autoFocus>
          {strings.welcome.start}
        </button>
      </div>
    </section>
  );
}

function Hardware({ page, onNext }: { page: ModelsPage; onNext: () => void }) {
  const s = strings.welcome;
  const hw = page.hardware;
  const [shown, setShown] = useState(0);
  useEffect(() => {
    // One row at a time, so the check reads as a check. CSS turns the
    // motion off for "reduce motion"; the rows still appear.
    const timer = window.setInterval(() => {
      setShown((n) => Math.min(n + 1, 3));
    }, 350);
    return () => {
      window.clearInterval(timer);
    };
  }, []);
  const rows: { icon: "gpu" | "ram" | "disk"; label: string; value: string }[] = [
    {
      icon: "gpu",
      label: s.gpu,
      value: hw.gpu ? `${hw.gpu.name}, ${gb(hw.gpu.vram_bytes)}` : s.noGpu,
    },
    { icon: "ram", label: s.ram, value: gb(hw.ram_bytes) },
    { icon: "disk", label: s.disk, value: gb(hw.disk_free_bytes) },
  ];
  const verdict = hw.tier === "high" ? s.tierHigh : hw.tier === "standard" ? s.tierStandard : s.tierBasic;
  return (
    <section className="welcome-section">
      <h2>{s.hardwareTitle}</h2>
      <ul className="check-list" aria-live="polite">
        {rows.slice(0, shown).map((row) => (
          <li key={row.icon} className="check-row">
            <Icon name={row.icon} />
            <span className="check-label">{row.label}</span>
            <span className="check-value">{row.value}</span>
          </li>
        ))}
      </ul>
      {shown >= 3 && (
        <>
          <p className="welcome-verdict">{verdict}</p>
          {hw.below_minimum && (
            <p className="welcome-warning">
              <Icon name="warning" />
              {s.belowMinimum}
            </p>
          )}
          <div className="safe-actions">
            <button type="button" className="button" onClick={onNext} autoFocus>
              {s.next}
            </button>
          </div>
        </>
      )}
    </section>
  );
}

/** Recommended, Lighter and Custom, each with its full download size. */
function Setup({
  page,
  engineBytes,
  choice,
  onPick,
  onBack,
  onNext,
}: {
  page: ModelsPage;
  engineBytes: number;
  choice: string | null;
  onPick: (model: string) => void;
  onBack: () => void;
  onNext: (model: string) => void;
}) {
  const s = strings.welcome;
  const fitting = page.models.filter((m) => m.in_catalog && m.fits);
  const recommended = fitting.find((m) => m.recommended) ?? fitting.at(0) ?? null;
  const lighter =
    fitting
      .filter((m) => recommended !== null && m.size_bytes < recommended.size_bytes)
      .sort((a, b) => b.size_bytes - a.size_bytes)
      .at(0) ?? null;
  const [custom, setCustom] = useState(false);
  const picked = choice ?? recommended?.id ?? null;
  useEffect(() => {
    if (choice === null && recommended) onPick(recommended.id);
  }, [choice, recommended, onPick]);

  const size = (m: ModelView) => {
    const bytes = engineBytes + (m.installed ? 0 : m.size_bytes);
    return bytes === 0 ? s.ready : s.download(gb(bytes));
  };
  const card = (m: ModelView, title: string, hint: string) => (
    <button
      type="button"
      key={title}
      className={picked === m.id && !custom ? "setup-card setup-card-picked" : "setup-card"}
      aria-pressed={picked === m.id && !custom}
      onClick={() => {
        setCustom(false);
        onPick(m.id);
      }}
    >
      <span className="setup-card-title">{title}</span>
      <span className="setup-card-model">{m.name}</span>
      <span className="field-hint">{hint}</span>
      <span className="setup-card-size">
        <Icon name="download" />
        {size(m)}
      </span>
    </button>
  );
  return (
    <section className="welcome-section">
      <h2>{s.setupTitle}</h2>
      <div className="setup-cards">
        {recommended && card(recommended, s.recommended, recommended.description)}
        {lighter && card(lighter, s.lighter, s.lighterHint)}
        <button
          type="button"
          className={custom ? "setup-card setup-card-picked" : "setup-card"}
          aria-pressed={custom}
          onClick={() => {
            setCustom(true);
          }}
        >
          <span className="setup-card-title">{s.custom}</span>
          <span className="field-hint">{s.customHint}</span>
        </button>
      </div>
      {custom && (
        <fieldset className="setup-custom">
          <legend className="sr-only">{s.custom}</legend>
          {fitting.map((m) => (
            <label key={m.id} className="setup-custom-row">
              <input
                type="radio"
                name="custom-model"
                checked={picked === m.id}
                onChange={() => {
                  onPick(m.id);
                }}
              />
              <span>{m.name}</span>
              <span className="field-hint">{size(m)}</span>
            </label>
          ))}
        </fieldset>
      )}
      <div className="safe-actions">
        <button type="button" className="button button-quiet" onClick={onBack}>
          {s.back}
        </button>
        <button
          type="button"
          className="button"
          disabled={picked === null}
          onClick={() => {
            if (picked) onNext(picked);
          }}
        >
          {s.next}
        </button>
      </div>
    </section>
  );
}

function Issues({ check }: { check: FolderCheck }) {
  const s = strings.welcome;
  return (
    <>
      {check.existing_library && <p className="field-hint">{s.existingLibrary}</p>}
      {check.issues.map((issue) => (
        <p key={issue} className={check.blocked ? "welcome-warning welcome-blocked" : "welcome-warning"}>
          <Icon name="warning" />
          {s.issues.get(issue) ?? issue}
        </p>
      ))}
    </>
  );
}

/** One folder, or a library folder and a separate models folder (Advanced). */
function Storage({
  setup,
  downloadBytes,
  onBack,
  onNext,
}: {
  setup: SetupState;
  downloadBytes: number;
  onBack: () => void;
  onNext: (next: SetupState) => void;
}) {
  const s = strings.welcome;
  const [library, setLibrary] = useState<string | null>(null);
  const [models, setModels] = useState<string | null>(null);
  // Each result is kept with the request it answers, so an old result
  // never counts for a new folder.
  const [libraryResult, setLibraryResult] = useState<{ key: string; check: FolderCheck } | null>(null);
  const [modelsResult, setModelsResult] = useState<{ key: string; check: FolderCheck } | null>(null);
  const [confirmedKey, setConfirmedKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<UiError | null>(null);
  const libraryPath = library ?? setup.default_library;
  const libraryKey = `${libraryPath}|${models === null ? "with-models" : "alone"}|${String(downloadBytes)}`;
  const modelsKey = `${models ?? ""}|${String(downloadBytes)}`;

  useEffect(() => {
    let active = true;
    // With no models folder of its own, the models go inside the library,
    // so the library must pass the models rules.
    void checkFolder(libraryPath, models === null ? "models" : "library", models === null ? downloadBytes : 0).then(
      (check) => {
        if (active) setLibraryResult({ key: libraryKey, check });
      },
    );
    return () => {
      active = false;
    };
  }, [libraryPath, models, downloadBytes, libraryKey]);

  useEffect(() => {
    let active = true;
    if (models !== null)
      void checkFolder(models, "models", downloadBytes).then((check) => {
        if (active) setModelsResult({ key: modelsKey, check });
      });
    return () => {
      active = false;
    };
  }, [models, downloadBytes, modelsKey]);

  const libraryCheck = libraryResult?.key === libraryKey ? libraryResult.check : null;
  const modelsCheck = models !== null && modelsResult?.key === modelsKey ? modelsResult.check : null;
  const bothKeys = `${libraryKey}#${modelsKey}`;
  const confirmed = confirmedKey === bothKeys;

  const checks = [libraryCheck, models === null ? null : modelsCheck].filter((c): c is FolderCheck => c !== null);
  const pending = libraryCheck === null || (models !== null && modelsCheck === null);
  const blocked = checks.some((c) => c.blocked);
  const warned = checks.some((c) => c.issues.length > 0);
  const choose = async (start: string, set: (path: string) => void) => {
    const path = await pickFolder(start);
    if (path !== null) set(path);
  };
  const shownFolder = (asked: string, check: FolderCheck | null) =>
    check && check.folder !== asked ? <p className="field-hint">{s.insideFolder(check.folder)}</p> : null;

  return (
    <section className="welcome-section">
      <h2>{s.storageTitle}</h2>
      <p className="muted">{s.storageIntro}</p>
      <div className="folder-row">
        <span className="field-label">{s.folder}</span>
        <code className="folder-path">{libraryPath}</code>
        <button
          type="button"
          className="button button-quiet"
          onClick={() => {
            void choose(libraryPath, setLibrary);
          }}
        >
          {s.change}
        </button>
        {library !== null && (
          <button
            type="button"
            className="link-button"
            onClick={() => {
              setLibrary(null);
            }}
          >
            {s.useDefault}
          </button>
        )}
      </div>
      {shownFolder(libraryPath, libraryCheck)}
      {libraryCheck && <Issues check={libraryCheck} />}
      <p className="field-hint">
        {s.total(gb(downloadBytes))}
        {libraryCheck && ` · ${s.free(gb(libraryCheck.free_bytes))}`}
      </p>
      <details className="more" open={models !== null}>
        <summary>{s.advanced}</summary>
        <p className="field-hint">{s.advancedHint}</p>
        <div className="folder-row">
          <span className="field-label">{s.modelsFolder}</span>
          <code className="folder-path">{models ?? `${libraryPath}\\models`}</code>
          <button
            type="button"
            className="button button-quiet"
            onClick={() => {
              void choose(models ?? libraryPath, setModels);
            }}
          >
            {s.change}
          </button>
          {models !== null && (
            <button
              type="button"
              className="link-button"
              onClick={() => {
                setModels(null);
              }}
            >
              {s.useDefault}
            </button>
          )}
        </div>
        {models !== null && shownFolder(models, modelsCheck)}
        {models !== null && modelsCheck && <Issues check={modelsCheck} />}
      </details>
      {pending && <p className="field-hint">{s.checking}</p>}
      {!pending && warned && !blocked && (
        <label className="check-confirm">
          <input
            type="checkbox"
            checked={confirmed}
            onChange={(e) => {
              setConfirmedKey(e.target.checked ? bothKeys : "");
            }}
          />
          {s.confirm}
        </label>
      )}
      {error && <ErrorNote error={error} />}
      <div className="safe-actions">
        <button type="button" className="button button-quiet" onClick={onBack}>
          {s.back}
        </button>
        <button
          type="button"
          className="button"
          disabled={pending || blocked || (warned && !confirmed) || busy}
          onClick={() => {
            setBusy(true);
            setError(null);
            setLocation(library, models, downloadBytes)
              .then(onNext)
              .catch((e: unknown) => {
                setError(toUiError(e));
              })
              .finally(() => {
                setBusy(false);
              });
          }}
        >
          {s.next}
        </button>
      </div>
    </section>
  );
}

function rowText(progress: EngineProgress | null, installed: boolean): string {
  const s = strings.welcome;
  if (installed) return s.done;
  if (!progress) return s.waiting;
  switch (progress.step) {
    case "downloading":
      return `${gb(progress.completed)} / ${gb(progress.total)}${
        progress.bytes_per_second > 0 ? ` · ${s.speed((progress.bytes_per_second / 1_000_000).toFixed(1))}` : ""
      }`;
    case "unpacking":
      return s.unpacking;
    case "checking":
      return s.checkingFiles;
    case "done":
      return s.done;
    case "stopped":
      return s.stopped;
    case "failed":
      // The error note below shows why, with the technical text under Details.
      return s.notDone;
  }
}

/** The engine, then the model, under one progress bar. */
function Downloads({
  mode,
  setup,
  model,
  onNext,
}: {
  mode: "full" | "engine";
  setup: SetupState;
  model: ModelView | null;
  onNext: () => void;
}) {
  const s = strings.welcome;
  const [engine, setEngine] = useState<EngineProgress | null>(setup.engine_progress);
  const [engineDone, setEngineDone] = useState(setup.engine_installed !== null);
  const [engineReady, setEngineReady] = useState(false);
  const [engineState, setEngineState] = useState<EngineStatus["state"] | null>(null);
  const [engineFailure, setEngineFailure] = useState<UiError | null>(null);
  const [modelProgress, setModelProgress] = useState<Download | null>(null);
  const [modelDone, setModelDone] = useState(model === null || model.installed);
  const [error, setError] = useState<UiError | null>(null);
  const [stopped, setStopped] = useState(false);
  const [notify, setNotifyState] = useState(true);
  const [tip, setTip] = useState(0);
  const started = useRef({ engine: false, model: false });

  const engineBytes = setup.engine_installed === null ? setup.engine_download_bytes : 0;
  const modelBytes = model && !model.installed ? model.size_bytes : 0;

  useEffect(() => {
    let heard = false;
    const showStatus = (status: EngineStatus) => {
      setEngineState(status.state);
      setEngineFailure(status.state === "error" ? { message: status.message, detail: status.detail } : null);
      setEngineReady(status.state === "ready" || status.state === "no_model" || status.state === "loading_model");
    };
    void getNotify().then(setNotifyState);
    const timer = window.setInterval(() => {
      setTip((n) => (n + 1) % s.tips.length);
    }, 7000);
    const subs = [
      onEngineSetup((p) => {
        setEngine(p);
        if (p?.step === "done") setEngineDone(true);
      }),
      onEngineSetupError((e) => {
        setError(e);
        setStopped(true);
      }),
      onDownload((d) => {
        setModelProgress(d);
        if (d?.state === "done") setModelDone(true);
        if (d?.state === "failed") {
          setError({ message: d.error || strings.models.failed(d.model), detail: d.error });
          setStopped(true);
        }
      }),
      onStatus((status) => {
        heard = true;
        showStatus(status);
      }),
    ];
    // Events only report changes, and the engine may have started before
    // this screen listened. A newer event wins over this first read.
    void getStatus().then((status) => {
      if (!heard) showStatus(status);
    });
    return () => {
      window.clearInterval(timer);
      for (const sub of subs)
        void sub.then((stop) => {
          stop();
        });
    };
  }, [s.tips.length]);

  // Start the engine install once, unless it already runs.
  useEffect(() => {
    if (engineDone || stopped || started.current.engine) return;
    const running = engine && ["downloading", "unpacking", "checking"].includes(engine.step);
    started.current.engine = true;
    if (!running) void engineInstall().catch((e: unknown) => {
      setError(toUiError(e));
      setStopped(true);
    });
  }, [engineDone, stopped, engine]);

  // Then the model, when the new engine answers.
  useEffect(() => {
    if (!model || modelDone || stopped || !engineDone || started.current.model) return;
    if (!engineReady && setup.engine_installed === null) return;
    started.current.model = true;
    void downloadModel(model.id).catch((e: unknown) => {
      const err = toUiError(e);
      // The new engine is still starting: try again on the next status.
      if (/not ready/i.test(err.message)) {
        started.current.model = false;
        setEngineReady(false);
        return;
      }
      setError(err);
      setStopped(true);
    });
  }, [model, modelDone, stopped, engineDone, engineReady, setup.engine_installed]);

  // Both done: make the model the default, then go on (once).
  const finished = useRef(false);
  useEffect(() => {
    if (!engineDone || !modelDone || finished.current) return;
    finished.current = true;
    const finish = model ? setDefaultModel(model.id).then(() => undefined) : Promise.resolve();
    void finish.then(onNext, (e: unknown) => {
      finished.current = false;
      setError(toUiError(e));
    });
  }, [engineDone, modelDone, model, onNext]);

  const engineSoFar = engineDone
    ? engineBytes
    : engine?.step === "downloading"
      ? engine.completed
      : engine && engine.step !== "stopped" && engine.step !== "failed"
        ? engineBytes
        : 0;
  const modelSoFar = modelDone ? modelBytes : (modelProgress?.completed ?? 0);
  const total = Math.max(1, engineBytes + modelBytes);
  // The engine is installed but could not start: show why, and Retry restarts it.
  const engineError = engineDone ? engineFailure : null;
  const anyError = error ?? engineError;
  const offline = anyError !== null && /internet/i.test(anyError.message) && mode === "full";
  const shownError = offline ? { message: s.offline, detail: anyError.detail } : anyError;

  const retry = () => {
    if (engineError) {
      setEngineFailure(null);
      setEngineState("starting");
      restartEngine().catch((e: unknown) => {
        setError(toUiError(e));
      });
    }
    setError(null);
    setStopped(false);
    started.current = { engine: false, model: false };
  };
  const stop = () => {
    setStopped(true);
    void engineInstallCancel();
    void stopDownload();
  };

  return (
    <section className="welcome-section">
      <h2>{mode === "engine" ? s.engineTitle : s.downloadTitle}</h2>
      <progress className="welcome-progress" max={total} value={engineSoFar + modelSoFar} aria-label={s.downloadTitle} />
      <ul className="check-list">
        <li className="check-row">
          <Icon name={engineDone ? "check" : "download"} />
          <span className="check-label">{s.engineRow}</span>
          <span className="check-value">{rowText(engine, engineDone)}</span>
        </li>
        {model && (
          <li className="check-row">
            <Icon name={modelDone ? "check" : "download"} />
            <span className="check-label">{s.modelRow(model.name)}</span>
            <span className="check-value">
              {modelDone
                ? s.done
                : modelProgress?.state === "downloading"
                  ? `${gb(modelProgress.completed)} / ${gb(modelProgress.total)}`
                  : modelProgress?.state === "checking"
                    ? s.checkingFiles
                    : engineDone && engineState === "starting"
                      ? s.engineStarting
                      : s.waiting}
            </span>
          </li>
        )}
      </ul>
      {shownError && (
        <ErrorNote error={shownError}>
          <button type="button" className="button" onClick={retry}>
            {s.retry}
          </button>
        </ErrorNote>
      )}
      {!shownError && (
        <div className="safe-actions">
          {stopped ? (
            <button type="button" className="button" onClick={retry}>
              {s.resume}
            </button>
          ) : (
            <button type="button" className="button button-quiet" onClick={stop}>
              {s.stop}
            </button>
          )}
        </div>
      )}
      <p className="field-hint">{s.closeHint}</p>
      <label className="check-confirm">
        <input
          type="checkbox"
          checked={notify}
          onChange={(e) => {
            setNotifyState(e.target.checked);
            void setNotify(e.target.checked);
          }}
        />
        {s.notifyLabel}
      </label>
      <p className="welcome-tip" aria-live="off">
        {s.tips.at(tip)}
      </p>
    </section>
  );
}

function PersonaStep({ onDone }: { onDone: () => void }) {
  const s = strings.welcome;
  const [personas, setPersonas] = useState<Persona[]>([]);
  const [own, setOwn] = useState(false);
  const [name, setName] = useState("");
  const [error, setError] = useState<UiError | null>(null);
  useEffect(() => {
    void listPersonas().then(setPersonas);
  }, []);
  const starters = useMemo(() => personas.slice(0, 4), [personas]);
  const use = (id: number | null) => {
    setDefaultPersona(id)
      .then(onDone)
      .catch((e: unknown) => {
        setError(toUiError(e));
      });
  };
  return (
    <section className="welcome-section">
      <h2>{s.personaTitle}</h2>
      <p className="muted">{s.personaIntro}</p>
      <div className="persona-picks">
        {starters.map((p) => (
          <button
            key={p.id}
            type="button"
            className="persona-pick"
            onClick={() => {
              use(p.id);
            }}
          >
            <Avatar kind="persona" owner={p} size="large" />
            <span>{p.name}</span>
          </button>
        ))}
        <button
          type="button"
          className={own ? "persona-pick setup-card-picked" : "persona-pick"}
          onClick={() => {
            setOwn(true);
          }}
        >
          <Avatar kind="persona" owner={null} label={s.makeOwn} size="large" />
          <span>{s.makeOwn}</span>
        </button>
      </div>
      {own && (
        <form
          className="persona-own"
          onSubmit={(e) => {
            e.preventDefault();
            const clean = name.trim();
            if (!clean) return;
            savePersona(null, { name: clean, description: "", appearance_tags: "", outfit: "", age: null }, { kind: "keep" })
              .then((p) => {
                use(p.id);
              })
              .catch((err: unknown) => {
                setError(toUiError(err));
              });
          }}
        >
          <label className="field">
            <span className="field-label">{s.yourName}</span>
            <input
              type="text"
              value={name}
              maxLength={40}
              autoFocus
              onChange={(e) => {
                setName(e.target.value);
              }}
            />
          </label>
          <button type="submit" className="button" disabled={!name.trim()}>
            {s.finish}
          </button>
        </form>
      )}
      {error && <ErrorNote error={error} />}
      <div className="safe-actions">
        <button
          type="button"
          className="button button-quiet"
          onClick={() => {
            use(null);
          }}
        >
          {s.skip}
        </button>
      </div>
    </section>
  );
}
