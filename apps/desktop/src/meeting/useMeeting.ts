import { useEffect, useState } from "react";
import type { ShellBridge } from "../shell/bridge";
import type { Advice, Answer, Gaps, ListeningState, Turn } from "../shell/types";
import { upsertTurn } from "./transcript";

export interface Levels {
  readonly seller: number;
  readonly customer: number;
}

const SILENT: Levels = { seller: 0, customer: 0 };

export function useListeningState(bridge: ShellBridge): ListeningState {
  const [state, setState] = useState<ListeningState>({ status: "idle" });
  useEffect(() => {
    let active = true;
    let sawEvent = false;
    const unsubscribe = bridge.subscribeMeeting((event) => {
      if (event.type !== "state") return;
      sawEvent = true;
      setState(event.state);
    });
    bridge.getListeningState().then(
      (initial) => {
        if (active && !sawEvent) setState(initial);
      },
      (error: unknown) => console.error("meeting_state failed", error),
    );
    return () => {
      active = false;
      unsubscribe();
    };
  }, [bridge]);
  return state;
}

/** What "what are we missing?" has come back with. */
export type GapsState =
  | { readonly status: "idle" }
  | { readonly status: "asking" }
  | { readonly status: "ready"; readonly gaps: Gaps }
  | { readonly status: "failed" };

/** What a palette request has come back with. */
export type AnswerState =
  | { readonly status: "idle" }
  | { readonly status: "asking" }
  | { readonly status: "ready"; readonly answer: Answer }
  | { readonly status: "failed" };

export interface MeetingFeed {
  readonly levels: Levels;
  readonly turns: readonly Turn[];
  readonly advice: Advice | null;
  readonly gaps: GapsState;
  dismissGaps(): void;
  readonly answer: AnswerState;
  dismissAnswer(): void;
}

/** Everything the overlay shows about the meeting. A new session starts clean. */
export function useMeetingFeed(bridge: ShellBridge): MeetingFeed {
  const [levels, setLevels] = useState<Levels>(SILENT);
  const [turns, setTurns] = useState<readonly Turn[]>([]);
  const [advice, setAdvice] = useState<Advice | null>(null);
  const [gaps, setGaps] = useState<GapsState>({ status: "idle" });
  const [answer, setAnswer] = useState<AnswerState>({ status: "idle" });
  useEffect(
    () =>
      bridge.subscribeMeeting((event) => {
        switch (event.type) {
          case "levels":
            setLevels({ seller: event.seller, customer: event.customer });
            break;
          case "turn":
            setTurns((current) => upsertTurn(current, event.turn));
            break;
          case "advice":
            setAdvice(event.advice);
            break;
          case "answerAsked":
            setAnswer({ status: "asking" });
            break;
          case "answer":
            setAnswer(event.answer ? { status: "ready", answer: event.answer } : { status: "failed" });
            break;
          case "gapsAsked":
            setGaps({ status: "asking" });
            break;
          case "gaps":
            setGaps(event.gaps ? { status: "ready", gaps: event.gaps } : { status: "failed" });
            break;
          case "state":
            if (event.state.status === "starting") {
              setTurns([]);
              setGaps({ status: "idle" });
              setAnswer({ status: "idle" });
            }
            if (event.state.status !== "listening") {
              setLevels(SILENT);
              // A suggestion only makes sense while the meeting is live.
              setAdvice(null);
            }
            break;
        }
      }),
    [bridge],
  );
  return {
    levels,
    turns,
    advice,
    gaps,
    dismissGaps: () => setGaps({ status: "idle" }),
    answer,
    dismissAnswer: () => setAnswer({ status: "idle" }),
  };
}
