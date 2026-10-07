import { useEffect, useState } from "react";
import type { ShellBridge } from "./bridge";
import type { Settings, ShellState, ShortcutBinding } from "./types";

export function useShellState(bridge: ShellBridge): ShellState | null {
  const [state, setState] = useState<ShellState | null>(null);
  useEffect(() => {
    let active = true;
    const unsubscribe = bridge.subscribe(setState);
    bridge.getState().then(
      (initial) => {
        // An event that raced ahead of this reply is newer; keep it.
        if (active) setState((current) => current ?? initial);
      },
      (error: unknown) => console.error("shell_state failed", error),
    );
    return () => {
      active = false;
      unsubscribe();
    };
  }, [bridge]);
  return state;
}

export function useShortcuts(bridge: ShellBridge): readonly ShortcutBinding[] {
  const [shortcuts, setShortcuts] = useState<readonly ShortcutBinding[]>([]);
  useEffect(() => {
    let active = true;
    bridge.getShortcuts().then(
      (bindings) => {
        if (active) setShortcuts(bindings);
      },
      (error: unknown) => console.error("shell_shortcuts failed", error),
    );
    return () => {
      active = false;
    };
  }, [bridge]);
  return shortcuts;
}

export function useSettings(bridge: ShellBridge): Settings | null {
  const [settings, setSettings] = useState<Settings | null>(null);
  useEffect(() => {
    let active = true;
    const unsubscribe = bridge.subscribeSettings(setSettings);
    bridge.getSettings().then(
      (initial) => {
        if (active) setSettings((current) => current ?? initial);
      },
      (error: unknown) => console.error("settings_get failed", error),
    );
    return () => {
      active = false;
      unsubscribe();
    };
  }, [bridge]);
  return settings;
}

/** How the session will translate, or null when translation is off. */
export function translationSummary(settings: Settings | null): string | null {
  if (settings === null || settings.myLanguage === settings.meetingLanguage) return null;
  const mine = settings.myLanguage.toUpperCase();
  const meeting = settings.meetingLanguage.toUpperCase();
  const incoming = settings.incomingTranslation !== "off";
  if (incoming && settings.translateMyVoice) return `${mine} ⇄ ${meeting}`;
  if (incoming) return `${meeting} → ${mine}`;
  return settings.translateMyVoice ? `${mine} → ${meeting}` : null;
}
