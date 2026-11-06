import { Component, useState, type ErrorInfo, type ReactNode } from "react";
import { diagnosticReport } from "../api";
import { strings } from "../strings";
import figure from "../assets/figure.png";

interface Caught {
  message: string;
  stack: string;
}

/**
 * Catches an error while a screen draws. Without it the window goes blank;
 * with it the user sees what happened and a way back. The data is safe: the
 * core saves every change at once, so a reload loses nothing.
 */
export class CrashGuard extends Component<{ children: ReactNode }, { caught: Caught | null }> {
  override state: { caught: Caught | null } = { caught: null };

  static getDerivedStateFromError(error: unknown) {
    const message = error instanceof Error ? `${error.name}: ${error.message}` : String(error);
    return { caught: { message, stack: "" } };
  }

  override componentDidCatch(error: unknown, info: ErrorInfo) {
    const stack = [error instanceof Error ? (error.stack ?? "") : "", info.componentStack ?? ""]
      .filter(Boolean)
      .join("\n");
    this.setState((s) => (s.caught ? { caught: { ...s.caught, stack } } : s));
  }

  override render() {
    return this.state.caught ? <CrashScreen caught={this.state.caught} /> : this.props.children;
  }
}

function CrashScreen({ caught }: { caught: Caught }) {
  const s = strings.crash;
  const [open, setOpen] = useState(false);
  const [copied, setCopied] = useState(false);
  // An Error's stack starts with its message already.
  const details = caught.stack.startsWith(caught.message)
    ? caught.stack
    : [caught.message, caught.stack].filter(Boolean).join("\n");
  const copy = () => {
    // The diagnostic report has the versions and the hardware. Without it,
    // the error alone still helps.
    void diagnosticReport()
      .catch(() => "")
      .then((report) => navigator.clipboard.writeText(report ? `${details}\n\n${report}` : details))
      .then(() => {
        setCopied(true);
      });
  };
  return (
    <main className="crash" role="alert">
      <img className="crash-figure" src={figure} alt="" />
      <h1>{s.title}</h1>
      <p>{s.body}</p>
      <div className="safe-actions">
        <button
          type="button"
          className="button"
          onClick={() => {
            window.location.reload();
          }}
        >
          {s.reload}
        </button>
        <button type="button" className="button button-quiet" onClick={copy}>
          {copied ? s.copied : s.copy}
        </button>
        <button
          type="button"
          className="link-button"
          aria-expanded={open}
          onClick={() => {
            setOpen((v) => !v);
          }}
        >
          {open ? strings.status.hideDetails : strings.status.details}
        </button>
      </div>
      {open && <pre className="error-detail crash-detail">{details}</pre>}
      <p className="muted">{s.help}</p>
    </main>
  );
}
