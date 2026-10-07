import { useEffect, useRef, useState } from "react";
import { sampleRecommendation, type OverlayRecommendation } from "../fixtures/sampleRecommendation";
import { formatClock } from "../meeting/transcript";
import { useListeningState, useMeetingFeed, type Levels } from "../meeting/useMeeting";
import type { ShellBridge } from "../shell/bridge";
import type { ListeningState, ShellState, ShortcutBinding, Turn } from "../shell/types";
import { translationSummary, useSettings, useShellState, useShortcuts } from "../shell/useShell";

export function Overlay({ bridge }: { bridge: ShellBridge }) {
  const state = useShellState(bridge);
  const shortcuts = useShortcuts(bridge);
  const listening = useListeningState(bridge);
  const { levels, turns } = useMeetingFeed(bridge);
  const translation = translationSummary(useSettings(bridge));
  const [sample, setSample] = useState<OverlayRecommendation | null>(null);
  if (state === null) return null;

  const expanded = state.overlayMode === "expanded";
  const active = listening.status === "listening" || listening.status === "starting";
  return (
    <div
      className="overlay"
      data-mode={state.overlayMode}
      data-click-through={state.clickThrough}
    >
      <StatusBar
        state={state}
        listening={listening}
        levels={levels}
        translation={translation}
        bridge={bridge}
      />
      {expanded && (
        <div className="overlay-body">
          {listening.status === "failed" && (
            <p className="failure" role="alert">
              {listening.message}
            </p>
          )}
          {sample ? (
            <Recommendation value={sample} />
          ) : turns.length > 0 ? (
            <Transcript turns={turns} />
          ) : active ? (
            <p className="muted">Waiting for someone to speak…</p>
          ) : (
            <Idle shortcuts={shortcuts} translation={translation} />
          )}
          <footer className="overlay-actions">
            <button
              type="button"
              className="action"
              data-tone={active ? "stop" : "start"}
              onClick={() => void (active ? bridge.stopListening() : bridge.startListening())}
            >
              {active ? "Stop listening" : "Start listening"}
            </button>
            {import.meta.env.DEV && (
              <button
                type="button"
                className="link"
                onClick={() => setSample(sample ? null : sampleRecommendation)}
              >
                {sample ? "Hide layout sample" : "Show layout sample"}
              </button>
            )}
          </footer>
        </div>
      )}
    </div>
  );
}

const STATUS_TEXT: Record<ListeningState["status"], string> = {
  idle: "Not listening",
  starting: "Starting…",
  listening: "Listening",
  failed: "Can't listen",
};

function StatusBar({
  state,
  listening,
  levels,
  translation,
  bridge,
}: {
  state: ShellState;
  listening: ListeningState;
  levels: Levels;
  translation: string | null;
  bridge: ShellBridge;
}) {
  const expanded = state.overlayMode === "expanded";
  return (
    <header className="status-bar" data-tauri-drag-region>
      <span className="status-dot" data-status={listening.status} aria-hidden="true" />
      <span className="brand">Aura</span>
      <span className="status-text" role="status">
        {STATUS_TEXT[listening.status]}
      </span>
      {listening.status === "listening" && (
        <span className="meters">
          <Meter label="Mic" value={levels.seller} />
          <Meter label="Meeting" value={levels.customer} />
        </span>
      )}
      {translation && (
        <span className="tag" title="Live translation. Change it from the menu bar.">
          {translation}
        </span>
      )}
      {state.clickThrough && <span className="tag">click-through</span>}
      <span className="spacer" />
      <button
        type="button"
        className="icon-button"
        aria-label="Open command palette"
        title="Command palette"
        onClick={() => void bridge.dispatch({ type: "togglePalette" })}
      >
        ⌘
      </button>
      <button
        type="button"
        className="icon-button"
        aria-label={expanded ? "Collapse overlay" : "Expand overlay"}
        aria-expanded={expanded}
        title={expanded ? "Collapse" : "Expand"}
        onClick={() => void bridge.dispatch({ type: "toggleOverlayMode" })}
      >
        <span className="chevron" data-open={expanded} aria-hidden="true" />
      </button>
    </header>
  );
}

function Meter({ label, value }: { label: string; value: number }) {
  return (
    <span className="meter" title={`${label} level`}>
      <span className="meter-label">{label}</span>
      <meter min={0} max={1} value={value} aria-label={`${label} level`} />
    </span>
  );
}

function Transcript({ turns }: { turns: readonly Turn[] }) {
  const end = useRef<HTMLDivElement>(null);
  const latest = turns[turns.length - 1];
  useEffect(() => {
    end.current?.scrollIntoView({ block: "end" });
  }, [latest?.id, latest?.text]);

  return (
    <ol className="transcript" aria-label="Transcript" aria-live="off">
      {turns.map((turn) => (
        <li key={turn.id} data-speaker={turn.speaker} data-final={turn.isFinal}>
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
      <div ref={end} />
    </ol>
  );
}

function Idle({
  shortcuts,
  translation,
}: {
  shortcuts: readonly ShortcutBinding[];
  translation: string | null;
}) {
  return (
    <>
      <section>
        <h2>Not listening</h2>
        <p className="muted">
          Start listening to transcribe your microphone and the meeting audio. Audio is streamed
          for transcription and is not saved.
        </p>
        <p className="muted">
          {translation
            ? `Translation is on (${translation}).`
            : "Translation is off."}{" "}
          Choose languages in the menu bar under Translation.
        </p>
      </section>
      <section>
        <h2>Shortcuts</h2>
        <ul className="shortcuts">
          {shortcuts.map((shortcut) => (
            <li key={shortcut.id} data-registered={shortcut.registered}>
              <span>{shortcut.label}</span>
              {shortcut.registered ? (
                <kbd>{shortcut.keys}</kbd>
              ) : (
                <span className="warning" title="Another app already owns this shortcut">
                  <kbd>{shortcut.keys}</kbd> unavailable
                </span>
              )}
            </li>
          ))}
        </ul>
      </section>
    </>
  );
}

function Recommendation({ value }: { value: OverlayRecommendation }) {
  return (
    <>
      <p className="sample-banner">Layout sample — not live output</p>
      <section>
        <h2>Now</h2>
        <p>{value.now}</p>
      </section>
      <section>
        <h2>Ask next</h2>
        <p className="primary">{value.askNext}</p>
      </section>
      <section>
        <h2>Why</h2>
        <p className="muted">{value.why}</p>
      </section>
    </>
  );
}
