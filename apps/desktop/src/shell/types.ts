// Mirrors `aura_core::shell` and `shortcuts::ShortcutBinding`. The Rust side
// is the source of truth; `wire_format_is_stable` pins the JSON shape.

export type OverlayMode = "collapsed" | "expanded";

export interface ShellState {
  readonly overlayMode: OverlayMode;
  readonly hidden: boolean;
  readonly clickThrough: boolean;
  readonly paletteOpen: boolean;
  readonly topicsVisible: boolean;
}

export type ShellCommand =
  | { readonly type: "toggleOverlayMode" }
  | { readonly type: "setOverlayMode"; readonly mode: OverlayMode }
  | { readonly type: "toggleHidden" }
  | { readonly type: "toggleClickThrough" }
  | { readonly type: "togglePalette" }
  | { readonly type: "closePalette" }
  | { readonly type: "toggleTopics" };

export interface ShortcutBinding {
  readonly id: string;
  readonly label: string;
  readonly keys: string;
  readonly registered: boolean;
}

export type WindowKind = "overlay" | "palette" | "topic" | "gallery" | "tokens" | "summary" | "sessions";

// Mirrors `aura_session` and `aura_core::transcript`.

export type Speaker = "seller" | "customer";

export interface Turn {
  readonly id: string;
  readonly speaker: Speaker;
  readonly text: string;
  readonly startMs: number;
  readonly endMs: number;
  readonly isFinal: boolean;
  /** What the interpreter said on the speaker's behalf, if anything. */
  readonly translation: string | null;
}

export type FailureReason =
  | "microphoneDenied"
  | "screenRecordingDenied"
  | "capture"
  | "service"
  | "credential";

export type ListeningState =
  | { readonly status: "idle" }
  | { readonly status: "starting" }
  | { readonly status: "listening" }
  | { readonly status: "failed"; readonly reason: FailureReason; readonly message: string };

// Mirrors `aura_core::topics`.

export type Zone = "topLeft" | "topRight" | "bottomLeft" | "bottomRight";
export type WindowSize = "small" | "medium" | "large" | "tall";

export interface TopicImage {
  readonly brief: string;
  readonly status: "pending" | "ready" | "failed";
}

export interface Source {
  readonly title: string;
  readonly url: string;
}

export interface Topic {
  readonly id: string;
  readonly title: string;
  readonly notes: readonly string[];
  readonly sourceTurnIds: readonly string[];
  /** Pages behind this topic's "Web:" notes. */
  readonly sources: readonly Source[];
  /** Mermaid source, when the agent drew a diagram. */
  readonly diagram: string | null;
  /** An illustration requested from the image sub-agent. */
  readonly image: TopicImage | null;
  readonly updatedAtMs: number;
  /** Null when the agent has put the topic away. */
  readonly placement: { readonly zone: Zone; readonly size: WindowSize } | null;
}

export type MeetingEvent =
  | { readonly type: "state"; readonly state: ListeningState }
  | { readonly type: "levels"; readonly seller: number; readonly customer: number }
  | { readonly type: "turn"; readonly turn: Turn }
  | { readonly type: "topics"; readonly topics: readonly Topic[] }
  | { readonly type: "advice"; readonly advice: Advice | null }
  /** The seller asked "what are we missing?"; the answer follows as `gaps`. */
  | { readonly type: "gapsAsked" }
  | { readonly type: "gaps"; readonly gaps: Gaps | null }
  /** The seller asked for something from the palette; `answer` follows. */
  | { readonly type: "answerAsked" }
  | { readonly type: "answer"; readonly answer: Answer | null };

/** Mirrors `settings::Settings`. Changed from the menu bar. */
export interface Settings {
  readonly myLanguage: string;
  readonly meetingLanguage: string;
  readonly incomingTranslation: "off" | "text" | "voice";
  readonly translateMyVoice: boolean;
  readonly webAccess: boolean;
  readonly illustrations: boolean;
}

// Mirrors `tokens`. A token's value never comes back from the core; only
// where the one in use comes from.

export type Provider = "openai" | "gemini";
export type TokenSource = "keychain" | "environment" | "none";

export interface TokenStatus {
  readonly openai: TokenSource;
  readonly gemini: TokenSource;
}

// Mirrors `aura_core::advice` and `aura_core::summary`.

/** The one next move for the seller. */
export interface Advice {
  readonly kind: "ask" | "say" | "caution";
  readonly text: string;
  readonly why: string;
  readonly sourceTurnIds: readonly string[];
  readonly atMs: number;
}

/** The advisor's answer to "what are we missing?". A suggestion, not a record. */
export interface Gaps {
  readonly understood: string;
  readonly missing: readonly string[];
  readonly priority: string;
}

export interface SummaryItem {
  readonly text: string;
  readonly sourceTurnIds: readonly string[];
}

export interface MeetingSummary {
  readonly headline: string;
  readonly overview: string;
  readonly environment: readonly SummaryItem[];
  readonly painPoints: readonly SummaryItem[];
  readonly requirements: readonly SummaryItem[];
  readonly openQuestions: readonly SummaryItem[];
  readonly commitments: readonly SummaryItem[];
  readonly nextStep: { readonly text: string; readonly why: string } | null;
}

export type SummaryState =
  | { readonly status: "none" }
  | { readonly status: "preparing" }
  | { readonly status: "ready"; readonly summary: MeetingSummary; readonly markdown: string }
  | { readonly status: "failed"; readonly message: string };

// Mirrors `aura_storage`.

export interface SessionMeta {
  readonly id: string;
  readonly title: string;
  readonly startedAtMs: number;
  readonly updatedAtMs: number;
  readonly turnCount: number;
  readonly topicCount: number;
  readonly durationMs: number;
  readonly hasSummary: boolean;
}

export interface SavedSession {
  readonly id: string;
  readonly title: string;
  readonly startedAtMs: number;
  readonly updatedAtMs: number;
  readonly turns: readonly Turn[];
  readonly topics: readonly Topic[];
  readonly summary: MeetingSummary | null;
}

/** A palette request. Mirrors `aura_intel::Ask`. */
export type Ask =
  | "askNext"
  | "explain"
  | "promised"
  | "summarize"
  | "environment"
  | "architecture"
  | "think"
  | "demo"
  | "canHelix"
  | "verify"
  | "answer"
  | "searchDocs"
  | "compare"
  | "question";

/** The reply to a palette request. Mirrors `aura_core::advice::Answer`. */
export interface Answer {
  readonly title: string;
  readonly summary: string;
  readonly points: readonly string[];
  readonly sayThis: string;
  readonly diagram: string | null;
  readonly verification: "verified" | "unverified" | "notApplicable";
  readonly sources: readonly Source[];
}
