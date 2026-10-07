import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type {
  Ask,
  ListeningState,
  MeetingEvent,
  Provider,
  SavedSession,
  SessionMeta,
  Settings,
  ShellCommand,
  ShellState,
  ShortcutBinding,
  SummaryState,
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
  /** Rejects with a message when there is no meeting to review. */
  askWhatsMissing(): Promise<void>;
  /** A palette request; `question` carries the seller's own words, if any. */
  ask(ask: Ask, question?: string): Promise<void>;
  getSummary(): Promise<SummaryState>;
  subscribeSummary(listener: (state: SummaryState) => void): () => void;
  openSummary(): Promise<void>;
  openSessions(): Promise<void>;
  listSessions(): Promise<SessionMeta[]>;
  getSession(id: string): Promise<SavedSession>;
  deleteSession(id: string): Promise<void>;
  /** Starts listening as a continuation of a saved session. */
  continueSession(id: string): Promise<void>;
  subscribeSessions(listener: () => void): () => void;
}

const STATE_EVENT = "shell://state";
const MEETING_EVENT = "meeting://event";
const SETTINGS_EVENT = "settings://changed";
const SUMMARY_EVENT = "summary://state";

const TOPIC_PREFIX = "topic-";

function windowKind(label: string | null): WindowKind {
  if (label === "palette" || label === "tokens" || label === "summary" || label === "sessions") {
    return label;
  }
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
    askWhatsMissing: () => invoke<void>("meeting_whats_missing"),
    ask: (ask, question) => invoke<void>("meeting_ask", { ask, question: question ?? null }),
    getSummary: () => invoke<SummaryState>("summary_get"),
    subscribeSummary(listener) {
      const unlisten = listen<SummaryState>(SUMMARY_EVENT, (event) => listener(event.payload));
      return () => {
        void unlisten.then((stop) => stop());
      };
    },
    openSummary: () => invoke<void>("summary_open"),
    openSessions: () => invoke<void>("sessions_open"),
    listSessions: () => invoke<SessionMeta[]>("sessions_list"),
    getSession: (id) => invoke<SavedSession>("session_get", { id }),
    deleteSession: (id) => invoke<void>("session_delete", { id }),
    continueSession: (id) => invoke<void>("session_continue", { id }),
    subscribeSessions(listener) {
      const unlisten = listen("sessions://changed", () => listener());
      return () => {
        void unlisten.then((stop) => stop());
      };
    },
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
    askWhatsMissing() {
      if (listening.status !== "listening") {
        return Promise.reject("Start listening first: there is no meeting to review yet.");
      }
      emitMeeting({ type: "gapsAsked" });
      window.setTimeout(() => emitMeeting({ type: "gaps", gaps: SAMPLE_GAPS }), 900);
      return Promise.resolve();
    },
    ask(ask) {
      if (listening.status !== "listening") {
        return Promise.reject("Start listening first: there is no meeting to work from yet.");
      }
      emitMeeting({ type: "answerAsked" });
      const verified = ask === "canHelix" || ask === "verify" || ask === "searchDocs";
      window.setTimeout(
        () =>
          emitMeeting({
            type: "answer",
            answer: {
              title: verified ? "OpenShift claim unverified" : "Current environment",
              summary: verified
                ? "I could not confirm “every OpenShift version” from the available BMC documentation."
                : "AWS and OpenShift underpin the environment; ServiceNow handles incidents.",
              points: verified
                ? ["The documentation describes OpenShift discovery, not blanket version support."]
                : ["Unknown where OpenShift runs.", "Unknown which platform hosts the CMDB."],
              sayThis: verified
                ? "Let me confirm the exact supported versions before I give you a definitive answer."
                : "",
              diagram: verified ? null : 'flowchart TD\nA["AWS"] --> C["CMDB"]\nB["OpenShift"] --> C',
              verification: verified ? "unverified" : "notApplicable",
              sources: verified ? [{ title: "BMC Documentation", url: "https://docs.bmc.com/docs/discovery" }] : [],
            },
          }),
        900,
      );
      return Promise.resolve();
    },
    getSummary: () => Promise.resolve(SAMPLE_SUMMARY),
    subscribeSummary: () => () => undefined,
    openSummary: () => Promise.resolve(),
    openSessions: () => Promise.resolve(),
    listSessions: () => Promise.resolve(sampleSessions.map(meta)),
    getSession(id) {
      const found = sampleSessions.find((session) => session.id === id);
      return found ? Promise.resolve(found) : Promise.reject("That session no longer exists.");
    },
    deleteSession(id) {
      sampleSessions = sampleSessions.filter((session) => session.id !== id);
      return Promise.resolve();
    },
    continueSession: () => Promise.resolve(),
    subscribeSessions: () => () => undefined,
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
        [
          2600,
          {
            type: "advice",
            advice: {
              kind: "ask",
              text: "How do you currently reconcile CI data from different sources?",
              why: "It decides whether discovery belongs in the picture.",
              sourceTurnIds: ["customer-0"],
              atMs: 9000,
            },
          },
        ],
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

const SAMPLE_GAPS = {
  understood: "Their CMDB goes stale and discovery is manual.",
  missing: [
    "Who owns the CMDB and who decides",
    "Number of configuration items and clusters",
    "Whether replacing ServiceNow is even in scope",
    "Target timeline",
  ],
  priority: "Ask who owns the CMDB and which sources feed it today.",
};

const SAMPLE_SUMMARY: SummaryState = {
  status: "ready",
  markdown: "# Discovery call: CMDB accuracy\n",
  summary: {
    headline: "Discovery call: CMDB accuracy and alert noise",
    overview:
      "The customer's CMDB becomes outdated quickly and nobody trusts it. They discover infrastructure with spreadsheets and an old script, and receive too many alerts from Dynatrace. The seller committed to a reference architecture by Friday.",
    environment: [
      { text: "Runs on AWS and OpenShift.", sourceTurnIds: ["customer-2"] },
      { text: "Uses ServiceNow for incidents and Dynatrace for monitoring.", sourceTurnIds: ["customer-2"] },
    ],
    painPoints: [{ text: "The CMDB gets outdated very quickly, and nobody trusts it.", sourceTurnIds: ["customer-1"] }],
    requirements: [],
    openQuestions: [{ text: "Which OpenShift versions need to be supported?", sourceTurnIds: ["customer-5"] }],
    commitments: [{ text: "Send a reference architecture by Friday.", sourceTurnIds: ["seller-8"] }],
    nextStep: {
      text: "Confirm the supported OpenShift versions, then send the reference architecture.",
      why: "Both were promised and the compatibility answer is still unverified.",
    },
  },
};

function meta(session: SavedSession): SessionMeta {
  return {
    id: session.id,
    title: session.title,
    startedAtMs: session.startedAtMs,
    updatedAtMs: session.updatedAtMs,
    turnCount: session.turns.length,
    topicCount: session.topics.length,
    durationMs: Math.max(0, ...session.turns.map((entry) => entry.endMs)),
    hasSummary: session.summary !== null,
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

const DAY_MS = 86_400_000;

let sampleSessions: SavedSession[] = [
  {
    id: "s-2",
    title: "Discovery call: CMDB accuracy and alert noise",
    startedAtMs: Date.now() - 3_600_000,
    updatedAtMs: Date.now() - 1_800_000,
    turns: [
      { id: "customer-0", speaker: "customer", text: "Honestly, our CMDB gets outdated very quickly. We run on AWS and OpenShift.", startMs: 1_000, endMs: 7_000, isFinal: true, translation: null },
      { id: "seller-0", speaker: "seller", text: "How are you currently discovering your infrastructure?", startMs: 9_000, endMs: 12_000, isFinal: true, translation: null },
      { id: "customer-1", speaker: "customer", text: "Mostly spreadsheets, and a script someone wrote years ago.", startMs: 14_000, endMs: 18_000, isFinal: true, translation: null },
    ],
    topics: [SAMPLE_TOPIC],
    summary: SAMPLE_SUMMARY.status === "ready" ? SAMPLE_SUMMARY.summary : null,
  },
  {
    id: "s-1",
    title: "Hello, can you hear me?",
    startedAtMs: Date.now() - 3 * DAY_MS,
    updatedAtMs: Date.now() - 3 * DAY_MS,
    turns: [
      { id: "customer-0", speaker: "customer", text: "Hello, can you hear me?", startMs: 500, endMs: 2_000, isFinal: true, translation: null },
    ],
    topics: [],
    summary: null,
  },
];
