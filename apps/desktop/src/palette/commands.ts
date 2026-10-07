import type { ListeningState, ShellCommand, ShellState } from "../shell/types";

export type PaletteAction =
  | { readonly kind: "shell"; readonly command: ShellCommand }
  | { readonly kind: "quit" }
  | { readonly kind: "listening"; readonly start: boolean }
  /** Listed so the palette shows where Aura is going; cannot be run yet. */
  | { readonly kind: "unavailable" };

export interface PaletteCommand {
  readonly id: string;
  readonly title: string;
  readonly group: "Aura" | "Controls";
  readonly action: PaletteAction;
}

const INTELLIGENCE_COMMANDS = [
  "What should I ask next?",
  "What are we missing?",
  "Explain this.",
  "Can Helix do this?",
  "Verify that.",
  "Search BMC docs.",
  "Give me an answer.",
  "Create architecture.",
  "Prepare demo.",
  "Compare with competitor.",
  "Show customer environment.",
  "What have we promised?",
  "Summarize so far.",
  "Think deeply.",
] as const;

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
    { id: "quit", title: "Quit Aura", group: "Controls", action: { kind: "quit" } },
  ];
  const intelligence = INTELLIGENCE_COMMANDS.map(
    (title, index): PaletteCommand => ({
      id: `aura-${index}`,
      title,
      group: "Aura",
      action: { kind: "unavailable" },
    }),
  );
  return [...windowCommands, ...intelligence];
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
