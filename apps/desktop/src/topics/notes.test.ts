import { describe, expect, it } from "vitest";
import { attentionOf, hostOf, parseNote } from "./notes";

describe("parseNote", () => {
  it("marks unanswered questions and promises", () => {
    expect(parseNote("Open: Does Discovery support OpenShift?")).toEqual({
      kind: "open",
      tag: "Open:",
      text: "Does Discovery support OpenShift?",
    });
    expect(parseNote("You promised: send the architecture").kind).toBe("promise");
  });

  it("does not mistake a promise for a plain seller note", () => {
    expect(parseNote("You: asked about discovery").kind).toBe("seller");
    expect(parseNote("you promised: a demo").kind).toBe("promise");
  });

  it("leaves unlabelled notes untouched", () => {
    expect(parseNote("  Runs AWS and OpenShift ")).toEqual({
      kind: "plain",
      tag: "",
      text: "Runs AWS and OpenShift",
    });
  });
});

describe("web notes", () => {
  it("are recognised so they can be shown apart from what was said", () => {
    expect(parseNote("Web: Docs describe an OpenShift provider.").kind).toBe("web");
  });

  it("show the site a source came from", () => {
    expect(hostOf("https://www.docs.bmc.com/docs/discovery/242/x.html?utm_source=openai")).toBe(
      "docs.bmc.com",
    );
    expect(hostOf("not a url")).toBe("");
  });
});

describe("attentionOf", () => {
  it("puts an unanswered question ahead of a promise", () => {
    expect(attentionOf(["You promised: a demo", "Open: Is OpenShift supported?"])).toBe("open");
    expect(attentionOf(["Customer: runs AWS", "You promised: a demo"])).toBe("promise");
    expect(attentionOf(["Customer: runs AWS"])).toBeNull();
  });
});
