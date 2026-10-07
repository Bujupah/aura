import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import type { ShellBridge } from "../shell/bridge";
import { useListeningState } from "../meeting/useMeeting";
import { useShellState } from "../shell/useShell";
import {
  filterCommands,
  isRunnable,
  nextRunnable,
  paletteCommands,
  type PaletteCommand,
} from "./commands";

export function Palette({ bridge }: { bridge: ShellBridge }) {
  const state = useShellState(bridge);
  const listening = useListeningState(bridge).status;
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const open = state?.paletteOpen ?? false;
  const commands = useMemo(
    () => (state ? filterCommands(paletteCommands(state, listening), query) : []),
    [state, listening, query],
  );
  const selectedIndex = commands.findIndex(
    (command) => command.id === selectedId && isRunnable(command),
  );
  const activeIndex = selectedIndex >= 0 ? selectedIndex : nextRunnable(commands, -1, 1);
  const active = commands[activeIndex];

  // Each opening starts clean: the seller is mid-meeting and should never
  // have to clear a stale query first.
  useEffect(() => {
    if (!open) return;
    setQuery("");
    setSelectedId(null);
    inputRef.current?.focus();
  }, [open]);

  useEffect(() => {
    const close = () => void bridge.dispatch({ type: "closePalette" });
    window.addEventListener("blur", close);
    return () => window.removeEventListener("blur", close);
  }, [bridge]);

  async function run(command: PaletteCommand) {
    switch (command.action.kind) {
      case "unavailable":
        return;
      case "quit":
        await bridge.quit();
        return;
      case "listening":
        // Starting can take a couple of seconds; don't hold the palette open.
        void (command.action.start ? bridge.startListening() : bridge.stopListening());
        await bridge.dispatch({ type: "closePalette" });
        return;
      case "shell":
        await bridge.dispatch(command.action.command);
        await bridge.dispatch({ type: "closePalette" });
    }
  }

  function onKeyDown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      void bridge.dispatch({ type: "closePalette" });
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const next = nextRunnable(commands, activeIndex, event.key === "ArrowDown" ? 1 : -1);
      setSelectedId(commands[next]?.id ?? null);
    } else if (event.key === "Enter" && active) {
      event.preventDefault();
      void run(active);
    }
  }

  if (state === null) return null;
  return (
    <div className="palette" onKeyDown={onKeyDown}>
      <input
        ref={inputRef}
        className="palette-input"
        value={query}
        onChange={(event) => setQuery(event.target.value)}
        placeholder="Ask Aura or run a command"
        aria-label="Command"
        role="combobox"
        aria-expanded="true"
        aria-controls="palette-list"
        aria-activedescendant={active ? `cmd-${active.id}` : undefined}
        autoComplete="off"
        spellCheck={false}
      />
      <ul id="palette-list" className="palette-list" role="listbox">
        {commands.length === 0 && <li className="palette-empty">No matching command</li>}
        {commands.map((command, index) => {
          const runnable = isRunnable(command);
          const heading = commands[index - 1]?.group !== command.group;
          return (
            <li key={command.id} role="presentation">
              {heading && <div className="palette-group">{command.group}</div>}
              <div
                id={`cmd-${command.id}`}
                role="option"
                className="palette-item"
                aria-selected={index === activeIndex}
                aria-disabled={!runnable}
                onMouseMove={() => runnable && setSelectedId(command.id)}
                onClick={() => void run(command)}
              >
                <span>{command.title}</span>
                {!runnable && <span className="palette-note">Not built yet</span>}
              </div>
            </li>
          );
        })}
      </ul>
      <footer className="palette-footer">
        <span>
          <kbd>↑</kbd>
          <kbd>↓</kbd> select
        </span>
        <span>
          <kbd>↩</kbd> run
        </span>
        <span>
          <kbd>esc</kbd> close
        </span>
      </footer>
    </div>
  );
}
