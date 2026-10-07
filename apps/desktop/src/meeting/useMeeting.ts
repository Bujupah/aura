import { useEffect, useState } from "react";
import type { ShellBridge } from "../shell/bridge";
import type { ListeningState, Turn } from "../shell/types";
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

/** Levels and transcript for the overlay. A new session starts a new transcript. */
export function useMeetingFeed(bridge: ShellBridge): { levels: Levels; turns: readonly Turn[] } {
  const [levels, setLevels] = useState<Levels>(SILENT);
  const [turns, setTurns] = useState<readonly Turn[]>([]);
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
          case "state":
            if (event.state.status === "starting") setTurns([]);
            if (event.state.status !== "listening") setLevels(SILENT);
            break;
        }
      }),
    [bridge],
  );
  return { levels, turns };
}
