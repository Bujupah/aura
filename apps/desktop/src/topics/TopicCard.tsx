import type { Topic } from "../shell/types";
import { Diagram } from "./Diagram";
import { attentionOf, hostOf, NOTE_LABELS, parseNote } from "./notes";

/** One topic window's contents. Presentation only. */
export function TopicCard({
  topic,
  imageUrl,
  onClose,
}: {
  topic: Topic;
  /** Object URL of the finished illustration, once fetched. */
  imageUrl: string | null;
  onClose: () => void;
}) {
  const attention = attentionOf(topic.notes);
  return (
    <article className="topic" data-attention={attention ?? "none"}>
      <header className="topic-bar" data-tauri-drag-region>
        <span className="topic-pip" aria-hidden="true" />
        <h1 dir="auto">{topic.title}</h1>
        {attention === "open" && <span className="topic-flag">open question</span>}
        <button
          type="button"
          className="icon-button"
          aria-label={`Close ${topic.title}`}
          title="Close"
          onClick={onClose}
        >
          ✕
        </button>
      </header>

      {topic.diagram !== null ? (
        <Diagram source={topic.diagram} />
      ) : (
        topic.image !== null && <Illustration topic={topic} imageUrl={imageUrl} />
      )}

      <ul className="topic-notes">
        {topic.notes.map((note) => {
          const parsed = parseNote(note);
          const label = NOTE_LABELS[parsed.kind];
          return (
            <li key={note} data-kind={parsed.kind}>
              {label && <span className="chip">{label}</span>}
              <span className="note-text" dir="auto">
                {parsed.text}
              </span>
            </li>
          );
        })}
      </ul>

      {topic.sources.length > 0 && (
        <footer className="topic-sources" title="Found on the web — check before relying on it">
          <span className="chip" data-kind="web">
            Source
          </span>
          {topic.sources.map((source, index) => (
            <span key={source.url} title={`${source.title}\n${source.url}`}>
              {index > 0 && ", "}
              {hostOf(source.url) || source.title}
            </span>
          ))}
        </footer>
      )}
    </article>
  );
}

function Illustration({ topic, imageUrl }: { topic: Topic; imageUrl: string | null }) {
  const status = topic.image?.status;
  if (status === "failed") {
    return <div className="visual visual-status">The illustration could not be made.</div>;
  }
  if (status !== "ready" || imageUrl === null) {
    return (
      <div className="visual visual-status" aria-busy="true">
        Drawing an illustration…
      </div>
    );
  }
  return (
    <figure className="visual illustration">
      <img src={imageUrl} alt={topic.image?.brief ?? ""} draggable={false} />
      <figcaption>AI illustration · not a record of what was said</figcaption>
    </figure>
  );
}
