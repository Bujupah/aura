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

export type WindowKind = "overlay" | "palette" | "topic" | "gallery" | "tokens";

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
  | { readonly type: "topics"; readonly topics: readonly Topic[] };

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
