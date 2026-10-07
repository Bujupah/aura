import { describe, expect, it } from "vitest";
import type { Settings } from "./types";
import { translationSummary } from "./useShell";

const base: Settings = {
  myLanguage: "en",
  meetingLanguage: "es",
  incomingTranslation: "off",
  translateMyVoice: false,
  webAccess: true,
  illustrations: true,
};

describe("translationSummary", () => {
  it("shows the direction that is switched on", () => {
    expect(translationSummary({ ...base, incomingTranslation: "text" })).toBe("ES → EN");
    expect(translationSummary({ ...base, translateMyVoice: true })).toBe("EN → ES");
    expect(translationSummary({ ...base, incomingTranslation: "voice", translateMyVoice: true })).toBe(
      "EN ⇄ ES",
    );
  });

  it("is absent when nothing is translated", () => {
    expect(translationSummary(base)).toBeNull();
    expect(translationSummary({ ...base, meetingLanguage: "en", incomingTranslation: "voice" })).toBeNull();
    expect(translationSummary(null)).toBeNull();
  });
});
