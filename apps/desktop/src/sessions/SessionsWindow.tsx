import { useCallback, useEffect, useState } from "react";
import { formatClock } from "../meeting/transcript";
import type { ShellBridge } from "../shell/bridge";
import type { SavedSession, SessionMeta } from "../shell/types";
import { SummaryBody } from "../summary/SummaryWindow";
import { parseNote } from "../topics/notes";
import { formatDuration, formatWhen } from "./format";

type Tab = "summary" | "notes" | "transcript";

function describe(error: unknown): string {
  return typeof error === "string" ? error : "That didn't work.";
}

/** Saved meetings: read one again, delete it, or pick it up where it ended. */
export function SessionsWindow({ bridge }: { bridge: ShellBridge }) {
  const [sessions, setSessions] = useState<readonly SessionMeta[] | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);

  const refresh = useCallback(() => {
    bridge.listSessions().then(
      (listed) => {
        setSessions(listed);
        setProblem(null);
        // Keep the selection if it still exists; otherwise show the newest.
        setSelectedId((current) =>
          listed.some((session) => session.id === current) ? current : (listed[0]?.id ?? null),
        );
      },
      (error: unknown) => setProblem(describe(error)),
    );
  }, [bridge]);

  useEffect(() => {
    refresh();
    return bridge.subscribeSessions(refresh);
  }, [bridge, refresh]);

  if (problem) return <main className="sessions-empty">{problem}</main>;
  if (sessions === null) return null;
  if (sessions.length === 0) {
    return (
      <main className="sessions-empty">
        <p>
          No saved sessions yet. A meeting is saved here when you stop listening, as long as Save
          Sessions is on in the menu bar.
        </p>
      </main>
    );
  }

  return (
    <main className="sessions">
      <ul className="session-list" aria-label="Sessions">
        {sessions.map((session) => (
          <li key={session.id}>
            <button
              type="button"
              aria-current={session.id === selectedId}
              onClick={() => setSelectedId(session.id)}
            >
              <span className="session-title" dir="auto">
                {session.title}
              </span>
              <span className="session-meta">
                {formatWhen(session.startedAtMs)} · {formatDuration(session.durationMs)}
              </span>
            </button>
          </li>
        ))}
      </ul>
      {selectedId && <Detail key={selectedId} bridge={bridge} id={selectedId} />}
    </main>
  );
}

function Detail({ bridge, id }: { bridge: ShellBridge; id: string }) {
  const [session, setSession] = useState<SavedSession | null>(null);
  const [tab, setTab] = useState<Tab>("summary");
  const [notice, setNotice] = useState<string | null>(null);
  const [confirmingDelete, setConfirmingDelete] = useState(false);

  useEffect(() => {
    let active = true;
    bridge.getSession(id).then(
      (loaded) => {
        if (!active) return;
        setSession(loaded);
        setTab(loaded.summary ? "summary" : loaded.topics.length > 0 ? "notes" : "transcript");
      },
      (error: unknown) => active && setNotice(describe(error)),
    );
    return () => {
      active = false;
    };
  }, [bridge, id]);

  if (session === null) {
    return <section className="sessions-empty">{notice ?? "Opening…"}</section>;
  }

  const tabs: readonly { id: Tab; label: string; shown: boolean }[] = [
    { id: "summary", label: "Summary", shown: session.summary !== null },
    { id: "notes", label: `Notes (${session.topics.length})`, shown: session.topics.length > 0 },
    { id: "transcript", label: `Transcript (${session.turns.length})`, shown: true },
  ];

  return (
    <section className="session-detail">
      <header className="session-head">
        <div>
          <h1 dir="auto">{session.title}</h1>
          <span className="session-meta">{formatWhen(session.startedAtMs)}</span>
          {notice && (
            <p className="warning" role="alert">
              {notice}
            </p>
          )}
        </div>
        <div className="session-actions">
          <button
            type="button"
            className="action"
            title="Start listening again with this transcript and these notes as the starting point"
            onClick={() => {
              setNotice(null);
              bridge.continueSession(session.id).catch((error: unknown) => setNotice(describe(error)));
            }}
          >
            Continue
          </button>
          {confirmingDelete ? (
            <>
              <button
                type="button"
                className="action"
                data-tone="danger"
                onClick={() => bridge.deleteSession(session.id).catch((error: unknown) => setNotice(describe(error)))}
              >
                Delete for good
              </button>
              <button type="button" className="action" onClick={() => setConfirmingDelete(false)}>
                Keep
              </button>
            </>
          ) : (
            <button type="button" className="action" data-tone="danger" onClick={() => setConfirmingDelete(true)}>
              Delete
            </button>
          )}
        </div>
      </header>
      <div className="session-tabs" role="tablist">
        {tabs
          .filter((entry) => entry.shown)
          .map((entry) => (
            <button
              key={entry.id}
              type="button"
              role="tab"
              aria-selected={tab === entry.id}
              onClick={() => setTab(entry.id)}
            >
              {entry.label}
            </button>
          ))}
      </div>
      <div className="session-panel" role="tabpanel">
        {tab === "summary" && session.summary && <SummaryBody summary={session.summary} />}
        {tab === "notes" && <Notes session={session} />}
        {tab === "transcript" && <SavedTranscript session={session} />}
      </div>
    </section>
  );
}

function Notes({ session }: { session: SavedSession }) {
  return (
    <div className="session-notes">
      {session.topics.map((topic) => (
        <section key={topic.id}>
          <h2 dir="auto">{topic.title}</h2>
          <ul>
            {topic.notes.map((note) => {
              const parsed = parseNote(note);
              return (
                <li key={note} dir="auto">
                  {parsed.tag && <span className="note-tag">{parsed.tag} </span>}
                  {parsed.text}
                </li>
              );
            })}
          </ul>
          {topic.sources.length > 0 && (
            <p className="session-meta">Web source: {topic.sources.map((source) => source.url).join(", ")}</p>
          )}
          {topic.diagram && <pre>{topic.diagram}</pre>}
        </section>
      ))}
    </div>
  );
}

function SavedTranscript({ session }: { session: SavedSession }) {
  return (
    <ol className="transcript" aria-label="Transcript">
      {session.turns.map((turn) => (
        <li key={turn.id} data-speaker={turn.speaker} data-final="true">
          <span className="turn-meta">
            <span className="turn-speaker">{turn.speaker === "seller" ? "Me" : "Customer"}</span>
            <time>{formatClock(turn.startMs)}</time>
          </span>
          <span className="turn-text" dir="auto">
            {turn.text}
          </span>
          {turn.translation && (
            <span className="turn-translation" dir="auto" title="What the meeting heard">
              {turn.translation}
            </span>
          )}
        </li>
      ))}
    </ol>
  );
}
