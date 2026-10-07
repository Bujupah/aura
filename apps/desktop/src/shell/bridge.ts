import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type {
  ListeningState,
  MeetingEvent,
  Provider,
  Settings,
  ShellCommand,
  ShellState,
  ShortcutBinding,
  TokenStatus,
  Topic,
  WindowKind,
} from "./types";

/** Everything the UI may ask of the desktop core. */
export interface ShellBridge {
  readonly windowKind: WindowKind;
  /** Set when this window shows one topic. */
  readonly topicId: string | null;
  getState(): Promise<ShellState>;
  getShortcuts(): Promise<ShortcutBinding[]>;
  dispatch(command: ShellCommand): Promise<ShellState>;
  subscribe(listener: (state: ShellState) => void): () => void;
  quit(): Promise<void>;
  getListeningState(): Promise<ListeningState>;
  startListening(): Promise<void>;
  stopListening(): Promise<void>;
  subscribeMeeting(listener: (event: MeetingEvent) => void): () => void;
  getTopics(): Promise<readonly Topic[]>;
  dismissTopic(id: string): Promise<void>;
  /** The topic's illustration as an object URL, or null if there is none. */
  getTopicImage(id: string): Promise<string | null>;
  getSettings(): Promise<Settings>;
  subscribeSettings(listener: (settings: Settings) => void): () => void;
  openTokens(): Promise<void>;
  getTokenStatus(): Promise<TokenStatus>;
  /** Rejects with a message the user can act on. */
  setToken(provider: Provider, value: string): Promise<TokenStatus>;
  clearToken(provider: Provider): Promise<TokenStatus>;
}

const STATE_EVENT = "shell://state";
const MEETING_EVENT = "meeting://event";
const SETTINGS_EVENT = "settings://changed";

const TOPIC_PREFIX = "topic-";

function windowKind(label: string | null): WindowKind {
  if (label === "palette" || label === "tokens") return label;
  return label?.startsWith(TOPIC_PREFIX) ? "topic" : "overlay";
}

function tauriBridge(): ShellBridge {
  const label = getCurrentWindow().label;
  return {
    windowKind: windowKind(label),
    topicId: label.startsWith(TOPIC_PREFIX) ? label.slice(TOPIC_PREFIX.length) : null,
    getState: () => invoke<ShellState>("shell_state"),
    getShortcuts: () => invoke<ShortcutBinding[]>("shell_shortcuts"),
    dispatch: (command) => invoke<ShellState>("shell_dispatch", { command }),
    subscribe(listener) {
      const unlisten = listen<ShellState>(STATE_EVENT, (event) => listener(event.payload));
      return () => {
        void unlisten.then((stop) => stop());
      };
    },
    quit: () => invoke<void>("app_quit"),
    getListeningState: () => invoke<ListeningState>("meeting_state"),
    startListening: () => invoke<void>("meeting_start"),
    stopListening: () => invoke<void>("meeting_stop"),
    subscribeMeeting(listener) {
      const unlisten = listen<MeetingEvent>(MEETING_EVENT, (event) => listener(event.payload));
      return () => {
        void unlisten.then((stop) => stop());
      };
    },
    getTopics: () => invoke<Topic[]>("topics_current"),
    dismissTopic: (id) => invoke<void>("topic_dismiss", { id }),
    async getTopicImage(id) {
      const bytes = await invoke<ArrayBuffer>("topic_image", { id });
      if (bytes.byteLength === 0) return null;
      return URL.createObjectURL(new Blob([bytes], { type: "image/png" }));
    },
    openTokens: () => invoke<void>("tokens_open"),
    getTokenStatus: () => invoke<TokenStatus>("tokens_status"),
    setToken: (provider, value) => invoke<TokenStatus>("token_set", { provider, value }),
    clearToken: (provider) => invoke<TokenStatus>("token_clear", { provider }),
    getSettings: () => invoke<Settings>("settings_get"),
    subscribeSettings(listener) {
      const unlisten = listen<Settings>(SETTINGS_EVENT, (event) => listener(event.payload));
      return () => {
        void unlisten.then((stop) => stop());
      };
    },
  };
}

/**
 * Stand-in for the desktop core so the UI can be developed in a plain browser
 * (`pnpm dev:web`, then `?window=palette`). It only approximates the real
 * state machine in `aura_core::shell`.
 */
function browserBridge(): ShellBridge {
  const query = new URLSearchParams(location.search);
  const requested = query.get("window");
  const kind: WindowKind =
    requested === "gallery" ? "gallery" : windowKind(requested === "topic" ? TOPIC_PREFIX : requested);
  let state: ShellState = {
    overlayMode: "collapsed",
    hidden: false,
    clickThrough: false,
    paletteOpen: kind === "palette",
    topicsVisible: true,
  };
  const listeners = new Set<(state: ShellState) => void>();
  const meetingListeners = new Set<(event: MeetingEvent) => void>();
  let tokenStatus: TokenStatus = { openai: "environment", gemini: "none" };
  let listening: ListeningState = { status: "idle" };
  let timers: number[] = [];

  function emitMeeting(event: MeetingEvent) {
    if (event.type === "state") listening = event.state;
    for (const listener of meetingListeners) listener(event);
  }

  function reduce(current: ShellState, command: ShellCommand): ShellState {
    switch (command.type) {
      case "toggleOverlayMode":
        return {
          ...current,
          overlayMode: current.overlayMode === "collapsed" ? "expanded" : "collapsed",
        };
      case "setOverlayMode":
        return { ...current, overlayMode: command.mode };
      case "toggleClickThrough":
        return { ...current, clickThrough: !current.clickThrough };
      case "toggleTopics":
        return { ...current, topicsVisible: !current.topicsVisible };
      case "toggleHidden":
      case "togglePalette":
      case "closePalette":
        return current;
    }
  }

  return {
    windowKind: kind,
    topicId: kind === "topic" ? (query.get("id") ?? SAMPLE_TOPIC.id) : null,
    getTopics: () => Promise.resolve([SAMPLE_TOPIC]),
    dismissTopic: () => Promise.resolve(),
    getTopicImage: () => Promise.resolve(null),
    getSettings: () =>
      Promise.resolve({
        myLanguage: "en",
        meetingLanguage: "es",
        incomingTranslation: "text",
        translateMyVoice: false,
        webAccess: true,
        illustrations: true,
      }),
    subscribeSettings: () => () => undefined,
    openTokens: () => Promise.resolve(),
    getTokenStatus: () => Promise.resolve(tokenStatus),
    setToken(provider, value) {
      if (value.trim() === "") return Promise.reject("Enter a token first.");
      tokenStatus = { ...tokenStatus, [provider]: "keychain" };
      return Promise.resolve(tokenStatus);
    },
    clearToken(provider) {
      tokenStatus = { ...tokenStatus, [provider]: provider === "openai" ? "environment" : "none" };
      return Promise.resolve(tokenStatus);
    },
    getState: () => Promise.resolve(state),
    getShortcuts: () =>
      Promise.resolve([
        { id: "palette", label: "Command palette", keys: "⌥Space", registered: true },
        { id: "hide", label: "Hide / restore all Aura windows", keys: "⌘⇧.", registered: true },
        { id: "overlay", label: "Expand / collapse overlay", keys: "⌥⇧Space", registered: false },
      ]),
    dispatch(command) {
      state = reduce(state, command);
      for (const listener of listeners) listener(state);
      return Promise.resolve(state);
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    quit: () => Promise.resolve(),
    getListeningState: () => Promise.resolve(listening),
    startListening() {
      emitMeeting({ type: "state", state: { status: "starting" } });
      const script: [number, MeetingEvent][] = [
        [400, { type: "state", state: { status: "listening" } }],
        [1200, turn("customer-0", "customer", "Honestly, our CMDB gets outdated", 1000, false)],
        [2200, turn("customer-0", "customer", "Honestly, our CMDB gets outdated very quickly. We run on AWS and OpenShift.", 1000, true)],
        [3200, turn("seller-0", "seller", "How are you currently discovering your infrastructure?", 9000, true)],
        [4200, turn("customer-1", "customer", "Mostly spreadsheets and", 14000, false)],
      ];
      timers = script.map(([delay, event]) => window.setTimeout(() => emitMeeting(event), delay));
      timers.push(
        window.setInterval(() => {
          if (listening.status === "listening") {
            emitMeeting({ type: "levels", seller: Math.random() * 0.4, customer: Math.random() });
          }
        }, 100),
      );
      return Promise.resolve();
    },
    stopListening() {
      for (const timer of timers) window.clearTimeout(timer);
      timers = [];
      emitMeeting({ type: "state", state: { status: "idle" } });
      return Promise.resolve();
    },
    subscribeMeeting(listener) {
      meetingListeners.add(listener);
      return () => meetingListeners.delete(listener);
    },
  };
}

export const SAMPLE_TOPIC: Topic = {
  id: "cmdb-accuracy",
  title: "CMDB accuracy",
  notes: [
    "Customer: CMDB gets outdated very quickly.",
    "Customer: Uses ServiceNow for incidents and Dynatrace for monitoring.",
    "Open: Does BMC Helix Discovery support OpenShift?",
    "Web: BMC documentation describes a Kubernetes/OpenShift Cluster discovery provider.",
  ],
  sourceTurnIds: ["customer-0"],
  sources: [
    {
      title: "Performing a discovery run - BMC Documentation",
      url: "https://docs.bmc.com/docs/discovery/242/performing-a-discovery-run-1313329980.html",
    },
  ],
  diagram: null,
  image: null,
  updatedAtMs: 14_000,
  placement: { zone: "topRight", size: "large" },
};

function turn(
  id: string,
  speaker: "seller" | "customer",
  text: string,
  startMs: number,
  isFinal: boolean,
): MeetingEvent {
  return {
    type: "turn",
    turn: {
      id,
      speaker,
      text,
      startMs,
      endMs: startMs + 4000,
      isFinal,
      translation: speaker === "seller" ? "¿Cómo descubren su infraestructura actualmente?" : null,
    },
  };
}

export function createShellBridge(): ShellBridge {
  return "__TAURI_INTERNALS__" in window ? tauriBridge() : browserBridge();
}
