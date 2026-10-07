import { describe, expect, it } from "vitest";
import type { Turn } from "../shell/types";
import { formatClock, MAX_TURNS, upsertTurn } from "./transcript";

function turn(id: string, startMs: number, text = id, isFinal = false): Turn {
  return { id, speaker: "customer", text, startMs, endMs: startMs + 1000, isFinal, translation: null };
}

describe("upsertTurn", () => {
  it("replaces a turn in place as it grows", () => {
    const first = upsertTurn([], turn("customer-0", 1000, "Our"));
    const second = upsertTurn(first, turn("customer-0", 1000, "Our CMDB", true));
    expect(second).toHaveLength(1);
    expect(second[0]).toMatchObject({ text: "Our CMDB", isFinal: true });
  });

  it("orders turns from different streams by when they started", () => {
    let turns = upsertTurn([], turn("customer-0", 1000));
    turns = upsertTurn(turns, turn("customer-1", 9000));
    // The seller's turn began earlier but its text arrived later.
    turns = upsertTurn(turns, turn("seller-0", 5000));
    expect(turns.map((t) => t.id)).toEqual(["customer-0", "seller-0", "customer-1"]);
  });

  it("keeps only the most recent turns", () => {
    let turns: Turn[] = [];
    for (let i = 0; i < MAX_TURNS + 5; i += 1) turns = upsertTurn(turns, turn(`t-${i}`, i * 1000));
    expect(turns).toHaveLength(MAX_TURNS);
    expect(turns[0]?.id).toBe("t-5");
  });
});

describe("formatClock", () => {
  it("formats meeting time as minutes and seconds", () => {
    expect(formatClock(0)).toBe("0:00");
    expect(formatClock(93_812)).toBe("1:33");
    expect(formatClock(3_600_000)).toBe("60:00");
  });
});
