import { describe, expect, it } from "vitest";
import type { ShellState } from "../shell/types";
import { filterCommands, isRunnable, nextRunnable, paletteCommands } from "./commands";

const state: ShellState = {
  overlayMode: "collapsed",
  hidden: false,
  clickThrough: false,
  paletteOpen: true,
  topicsVisible: true,
};

describe("paletteCommands", () => {
  it("labels toggles from the current state", () => {
    const titles = (s: ShellState) => paletteCommands(s, "idle").map((c) => c.title);
    expect(titles(state)).toContain("Expand overlay");
    expect(titles({ ...state, overlayMode: "expanded" })).toContain("Collapse overlay");
    expect(titles({ ...state, clickThrough: true })).toContain("Turn off click-through");
  });

  it("offers to start or stop listening depending on the session", () => {
    const first = (status: "idle" | "starting" | "listening" | "failed") =>
      paletteCommands(state, status)[0]?.title;
    expect(first("idle")).toBe("Start listening");
    expect(first("failed")).toBe("Start listening");
    expect(first("starting")).toBe("Stop listening");
    expect(first("listening")).toBe("Stop listening");
  });

  it("offers only the intelligence that is actually built", () => {
    const aura = paletteCommands(state, "idle").filter((c) => c.group === "Aura");
    const runnable = aura.filter(isRunnable).map((c) => c.title);
    expect(runnable).toEqual(["What are we missing?"]);
    expect(aura.length).toBeGreaterThan(runnable.length);
  });
});

describe("filterCommands", () => {
  const commands = paletteCommands(state, "idle");

  it("returns everything for a blank query", () => {
    expect(filterCommands(commands, "   ")).toHaveLength(commands.length);
  });

  it("matches all terms in any order, ignoring case", () => {
    expect(filterCommands(commands, "MISSING what").map((c) => c.title)).toEqual([
      "What are we missing?",
    ]);
  });

  it("returns nothing when a term is absent", () => {
    expect(filterCommands(commands, "overlay zzz")).toEqual([]);
  });
});

describe("nextRunnable", () => {
  const commands = paletteCommands(state, "idle");

  it("skips unavailable commands and wraps", () => {
    const last = commands.findLastIndex(isRunnable);
    const first = commands.findIndex(isRunnable);
    expect(nextRunnable(commands, last, 1)).toBe(first);
    expect(nextRunnable(commands, first, -1)).toBe(last);
  });

  it("reports -1 when nothing is runnable", () => {
    const unavailable = commands.filter((c) => !isRunnable(c));
    expect(nextRunnable(unavailable, 0, 1)).toBe(-1);
    expect(nextRunnable([], -1, 1)).toBe(-1);
  });
});
