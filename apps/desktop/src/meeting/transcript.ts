import type { Turn } from "../shell/types";

/** The overlay shows recent conversation, not a full record. */
export const MAX_TURNS = 200;

/**
 * Applies a turn update. Turns arrive from two independent streams, and each
 * one is updated in place until final, so the list is kept in the order the
 * turns started.
 */
export function upsertTurn(turns: readonly Turn[], turn: Turn): Turn[] {
  const next = turns.filter((existing) => existing.id !== turn.id);
  const index = next.findIndex((existing) => existing.startMs > turn.startMs);
  next.splice(index === -1 ? next.length : index, 0, turn);
  return next.slice(-MAX_TURNS);
}

/** `93812` → `1:33`. */
export function formatClock(ms: number): string {
  const totalSeconds = Math.floor(ms / 1000);
  const minutes = Math.floor(totalSeconds / 60);
  return `${minutes}:${String(totalSeconds % 60).padStart(2, "0")}`;
}
