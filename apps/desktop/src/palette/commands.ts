import type { Ask, ListeningState, ShellCommand, ShellState } from "../shell/types";

export type PaletteAction =
  | { readonly kind: "shell"; readonly command: ShellCommand }
  | { readonly kind: "quit" }
  | { readonly kind: "tokens" }
  | { readonly kind: "missing" }
  /** One of the advisor's on-request answers. */
  | { readonly kind: "ask"; readonly ask: Ask; readonly question?: string }
  | { readonly kind: "summary" }
  | { readonly kind: "sessions" }
  | { readonly kind: "listening"; readonly start: boolean }
  /** Listed so the palette shows where Aura is going; cannot be run yet. */
  | { readonly kind: "unavailable" };

export interface PaletteCommand {
  readonly id: string;
  readonly title: string;
  readonly group: "Aura" | "Controls";
  readonly action: PaletteAction;
}

/** What the seller can ask Aura for while a meeting is under way. */
const REQUESTS: readonly (readonly [title: string, ask: Ask])[] = [
  ["What should I ask next?", "askNext"],
  ["Explain this.", "explain"],
  ["Can Helix do this?", "canHelix"],
  ["Verify that.", "verify"],
  ["Search BMC docs.", "searchDocs"],
  ["Give me an answer.", "answer"],
  ["Create architecture.", "architecture"],
  ["Prepare demo.", "demo"],
  ["Compare with competitor.", "compare"],
  ["Show customer environment.", "environment"],
  ["What have we promised?", "promised"],
  ["Summarize so far.", "summarize"],
  ["Think deeply.", "think"],
];

export function paletteCommands(
  state: ShellState,
  listening: ListeningState["status"],
): PaletteCommand[] {
  const active = listening === "listening" || listening === "starting";
  const windowCommands: PaletteCommand[] = [
    {
      id: "toggle-listening",
      title: active ? "Stop listening" : "Start listening",
      group: "Controls",
      action: { kind: "listening", start: !active },
    },
    {
      id: "toggle-overlay",
      title: state.overlayMode === "collapsed" ? "Expand overlay" : "Collapse overlay",
      group: "Controls",
      action: { kind: "shell", command: { type: "toggleOverlayMode" } },
    },
    {
      id: "toggle-topics",
      title: state.topicsVisible ? "Hide topic windows" : "Show topic windows",
      group: "Controls",
      action: { kind: "shell", command: { type: "toggleTopics" } },
    },
    {
      id: "toggle-click-through",
      title: state.clickThrough ? "Turn off click-through" : "Turn on click-through",
      group: "Controls",
      action: { kind: "shell", command: { type: "toggleClickThrough" } },
    },
    {
      id: "hide",
      title: "Hide all Aura windows",
      group: "Controls",
      action: { kind: "shell", command: { type: "toggleHidden" } },
    },
    { id: "sessions", title: "Open sessions…", group: "Controls", action: { kind: "sessions" } },
    { id: "summary", title: "Show last meeting summary", group: "Controls", action: { kind: "summary" } },
    { id: "tokens", title: "Set API tokens…", group: "Controls", action: { kind: "tokens" } },
    { id: "quit", title: "Quit Aura", group: "Controls", action: { kind: "quit" } },
  ];
  const requests: PaletteCommand[] = [
    { id: "missing", title: "What are we missing?", group: "Aura", action: { kind: "missing" } },
    ...REQUESTS.map(
      ([title, ask]): PaletteCommand => ({ id: `ask-${ask}`, title, group: "Aura", action: { kind: "ask", ask } }),
    ),
  ];
  return [...windowCommands, ...requests];
}

/** Case-insensitive match on every whitespace-separated term, in any order. */
export function filterCommands(
  commands: readonly PaletteCommand[],
  query: string,
): PaletteCommand[] {
  const terms = query.toLowerCase().split(/\s+/).filter(Boolean);
  return commands.filter((command) => {
    const title = command.title.toLowerCase();
    return terms.every((term) => title.includes(term));
  });
}

/** Anything typed that is not a command can be put to Aura as a question. */
const MIN_QUESTION_CHARS = 4;

/**
 * The commands matching `query`, followed by an offer to ask Aura the query
 * itself when it reads like a question rather than a command name.
 */
export function commandsFor(commands: readonly PaletteCommand[], query: string): PaletteCommand[] {
  const matches = filterCommands(commands, query);
  const question = query.trim();
  if (question.length < MIN_QUESTION_CHARS) return matches;
  return [
    ...matches,
    {
      id: "ask-question",
      title: `Ask Aura: “${question}”`,
      group: "Aura",
      action: { kind: "ask", ask: "question", question },
    },
  ];
}

export function isRunnable(command: PaletteCommand): boolean {
  return command.action.kind !== "unavailable";
}

/** Next runnable index from `from` in `direction`, wrapping; -1 if none. */
export function nextRunnable(
  commands: readonly PaletteCommand[],
  from: number,
  direction: 1 | -1,
): number {
  for (let step = 1; step <= commands.length; step += 1) {
    const index = (from + direction * step + commands.length * step) % commands.length;
    const candidate = commands[index];
    if (candidate && isRunnable(candidate)) return index;
  }
  return -1;
}
