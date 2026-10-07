import { describe, expect, it } from "vitest";
import type { ShellState } from "../shell/types";
import { commandsFor, filterCommands, isRunnable, nextRunnable, paletteCommands } from "./commands";

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

  it("offers every request the advisor can answer", () => {
    const aura = paletteCommands(state, "idle").filter((c) => c.group === "Aura");
    expect(aura.every(isRunnable)).toBe(true);
    expect(aura.map((c) => c.title)).toContain("Verify that.");
    expect(aura[0]?.title).toBe("What are we missing?");
  });
});

describe("commandsFor", () => {
  const commands = paletteCommands(state, "idle");

  it("offers to put free text to Aura as a question", () => {
    const offered = commandsFor(commands, "do they have a budget?");
    expect(offered).toHaveLength(1);
    expect(offered[0]?.action).toEqual({ kind: "ask", ask: "question", question: "do they have a budget?" });
  });

  it("lists matching commands before the question", () => {
    const offered = commandsFor(commands, "verify");
    expect(offered.map((c) => c.title)).toEqual(["Verify that.", "Ask Aura: “verify”"]);
  });

  it("does not offer a question for a blank or tiny query", () => {
    expect(commandsFor(commands, "")).toHaveLength(commands.length);
    expect(commandsFor(commands, "ex").some((c) => c.id === "ask-question")).toBe(false);
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

  it("reports -1 when there is nothing to select", () => {
    expect(nextRunnable([], -1, 1)).toBe(-1);
  });
});
