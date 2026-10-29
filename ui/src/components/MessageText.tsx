// Shows message text as text. Actions in *asterisks* show in italic.
// No HTML, no links, no images: model output and imported cards are untrusted.

const ACTION = /(\*[^*\n]+\*)/g;

export function MessageText({ text }: { text: string }) {
  const parts = text.split(ACTION);
  return (
    <p className="message-text">
      {parts.map((part, i) =>
        part.length > 2 && part.startsWith("*") && part.endsWith("*") ? (
          <em key={i} className="action">
            {part.slice(1, -1)}
          </em>
        ) : (
          <span key={i}>{part}</span>
        ),
      )}
    </p>
  );
}
