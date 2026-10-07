export type NoteKind = "open" | "promise" | "web" | "customer" | "seller" | "plain";

export interface ParsedNote {
  readonly kind: NoteKind;
  /** The leading label as written, such as "Customer:", or "" if none. */
  readonly tag: string;
  readonly text: string;
}

const TAGS: readonly (readonly [prefix: string, kind: NoteKind])[] = [
  ["open:", "open"],
  ["you promised:", "promise"],
  ["web:", "web"],
  ["customer:", "customer"],
  ["you:", "seller"],
];

/**
 * Notes are plain sentences, some starting with a conventional label. The
 * label is split off so unanswered questions and promises can be marked.
 */
export function parseNote(note: string): ParsedNote {
  const trimmed = note.trim();
  const lower = trimmed.toLowerCase();
  for (const [prefix, kind] of TAGS) {
    if (lower.startsWith(prefix)) {
      return {
        kind,
        tag: trimmed.slice(0, prefix.length),
        text: trimmed.slice(prefix.length).trim(),
      };
    }
  }
  return { kind: "plain", tag: "", text: trimmed };
}

/** `https://docs.bmc.com/docs/x.html` → `docs.bmc.com`; "" if not a URL. */
export function hostOf(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return "";
  }
}

/** The short label shown in front of a note of each kind; "" for none. */
export const NOTE_LABELS: Record<NoteKind, string> = {
  open: "Open",
  promise: "Promised",
  web: "Web",
  customer: "Customer",
  seller: "You",
  plain: "",
};

/** What most deserves the seller's eye in a window, if anything. */
export function attentionOf(notes: readonly string[]): "open" | "promise" | null {
  const kinds = notes.map((note) => parseNote(note).kind);
  if (kinds.includes("open")) return "open";
  return kinds.includes("promise") ? "promise" : null;
}
