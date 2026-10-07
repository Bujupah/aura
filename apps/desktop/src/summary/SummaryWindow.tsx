import { useEffect, useState } from "react";
import type { ShellBridge } from "../shell/bridge";
import type { MeetingSummary, SummaryItem, SummaryState } from "../shell/types";

function useSummary(bridge: ShellBridge): SummaryState | null {
  const [state, setState] = useState<SummaryState | null>(null);
  useEffect(() => {
    let active = true;
    let sawEvent = false;
    const unsubscribe = bridge.subscribeSummary((next) => {
      sawEvent = true;
      setState(next);
    });
    bridge.getSummary().then(
      (initial) => {
        if (active && !sawEvent) setState(initial);
      },
      (error: unknown) => console.error("summary_get failed", error),
    );
    return () => {
      active = false;
      unsubscribe();
    };
  }, [bridge]);
  return state;
}

export function SummaryWindow({ bridge }: { bridge: ShellBridge }) {
  const state = useSummary(bridge);
  const [copied, setCopied] = useState(false);
  if (state === null) return null;

  if (state.status !== "ready") {
    return (
      <main className="summary">
        <div className="summary-status" role="status">
          {state.status === "preparing" && "Writing up the meeting…"}
          {state.status === "none" && "The summary appears here when a meeting ends."}
          {state.status === "failed" && `The summary could not be written. ${state.message}`}
        </div>
      </main>
    );
  }

  async function copy(markdown: string) {
    try {
      await navigator.clipboard.writeText(markdown);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    } catch (error) {
      console.error("copy failed", error);
    }
  }

  return (
    <main className="summary">
      <SummaryBody summary={state.summary} />
      <footer className="summary-footer">
        <span>Kept with the session when Save Sessions is on. Find it later under Sessions.</span>
        <button type="button" className="action" onClick={() => void copy(state.markdown)}>
          {copied ? "Copied" : "Copy as Markdown"}
        </button>
      </footer>
    </main>
  );
}

export function SummaryBody({ summary }: { summary: MeetingSummary }) {
  return (
    <article className="summary-body">
      <header>
        <h1 dir="auto">{summary.headline}</h1>
      </header>
      <p dir="auto">{summary.overview}</p>
      <Section title="Customer environment" items={summary.environment} />
      <Section title="Pain points" items={summary.painPoints} />
      <Section title="Requirements" items={summary.requirements} />
      <Section title="Open questions" items={summary.openQuestions} />
      <Section title="Commitments" items={summary.commitments} />
      {summary.nextStep && (
        <section className="summary-next">
          <h2>Suggested next step</h2>
          <p dir="auto">{summary.nextStep.text}</p>
          {summary.nextStep.why && (
            <p className="muted" dir="auto">
              {summary.nextStep.why}
            </p>
          )}
        </section>
      )}
    </article>
  );
}

function Section({ title, items }: { title: string; items: readonly SummaryItem[] }) {
  if (items.length === 0) return null;
  return (
    <section>
      <h2>{title}</h2>
      <ul>
        {items.map((item) => (
          <li key={item.text} dir="auto">
            {item.text}
          </li>
        ))}
      </ul>
    </section>
  );
}
