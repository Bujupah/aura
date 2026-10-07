import { describe, expect, it } from "vitest";
import { formatDuration, formatWhen } from "./format";

describe("formatDuration", () => {
  it("uses the largest sensible unit", () => {
    expect(formatDuration(45_000)).toBe("45 s");
    expect(formatDuration(12 * 60_000)).toBe("12 min");
    expect(formatDuration(65 * 60_000)).toBe("1 h 05 min");
  });
});

describe("formatWhen", () => {
  const now = new Date(2026, 9, 7, 15, 0).getTime();

  it("names today and yesterday", () => {
    expect(formatWhen(new Date(2026, 9, 7, 9, 30).getTime(), now)).toMatch(/^Today /);
    expect(formatWhen(new Date(2026, 9, 6, 23, 50).getTime(), now)).toMatch(/^Yesterday /);
  });

  it("gives the date for anything older", () => {
    const older = formatWhen(new Date(2026, 9, 3, 16, 20).getTime(), now);
    expect(older).not.toMatch(/Today|Yesterday/);
    expect(older).toContain("2026");
  });
});
