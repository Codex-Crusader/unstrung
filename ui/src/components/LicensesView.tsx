import { useState } from "react";
import { strings } from "../strings";

interface License {
  name: string;
  text: string;
  used_by: string[];
}
interface Engine {
  name: string;
  license: string;
  url: string;
  note: string;
}
interface Data {
  app: string;
  licenses: License[];
  engines: Engine[];
}

/**
 * Settings > Help: the app's license and the licenses of what it uses. The
 * data (tools/licenses.py writes it) loads when the user opens the list, so
 * the main bundle stays small.
 */
export function LicensesView() {
  const s = strings.licenses;
  const [data, setData] = useState<Data | null>(null);
  const [failed, setFailed] = useState(false);
  return (
    <details
      className="licenses"
      onToggle={(e) => {
        if (!e.currentTarget.open || data) return;
        import("../licenses.json")
          .then((m) => {
            setData(m.default);
          })
          .catch(() => {
            setFailed(true);
          });
      }}
    >
      <summary>{s.title}</summary>
      {failed && <p className="field-hint warn">{s.failed}</p>}
      {!failed && data === null && <p className="field-hint">{s.loading}</p>}
      {data && (
        <>
          <h3>{s.app}</h3>
          <pre>{data.app}</pre>
          <h3>{s.engines}</h3>
          <p className="field-hint">{s.enginesHint}</p>
          <ul>
            {data.engines.map((e) => (
              <li key={e.name}>
                {e.name} ({e.license}): {e.note} {e.url}
              </li>
            ))}
          </ul>
          <h3>{s.libraries}</h3>
          <p className="field-hint">{s.librariesHint(data.licenses.length)}</p>
          {data.licenses.map((l) => (
            <details key={`${l.name} ${l.used_by[0] ?? ""}`}>
              <summary>
                {l.name}: {l.used_by.join(", ")}
              </summary>
              <pre>{l.text}</pre>
            </details>
          ))}
        </>
      )}
    </details>
  );
}
