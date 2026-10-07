import { useEffect, useId, useState } from "react";

type Rendered = { readonly svg: string } | { readonly error: true } | null;

/**
 * Draws Mermaid source written by the note-taking agent. The source is
 * untrusted: Mermaid runs in strict mode, which sanitizes the output and
 * disables links and scripts. The library is loaded only when a diagram is
 * actually shown.
 */
export function Diagram({ source }: { source: string }) {
  const id = `diagram-${useId().replace(/[^a-zA-Z0-9]/g, "")}`;
  const [rendered, setRendered] = useState<Rendered>(null);

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        const { default: mermaid } = await import("mermaid");
        const dark = window.matchMedia("(prefers-color-scheme: dark)").matches;
        mermaid.initialize({
          startOnLoad: false,
          securityLevel: "strict",
          theme: dark ? "dark" : "neutral",
          fontFamily: "-apple-system, BlinkMacSystemFont, sans-serif",
          flowchart: { useMaxWidth: true, htmlLabels: false, padding: 10 },
        });
        // `parse` rejects bad source without leaving error markup in the page.
        if ((await mermaid.parse(source, { suppressErrors: true })) === false) {
          throw new Error("not valid Mermaid");
        }
        const { svg } = await mermaid.render(id, source);
        if (active) setRendered({ svg });
      } catch (error) {
        console.warn("diagram could not be drawn", error);
        if (active) setRendered({ error: true });
      }
    })();
    return () => {
      active = false;
    };
  }, [id, source]);

  if (rendered === null) return <div className="visual visual-status">Drawing diagram…</div>;
  if ("error" in rendered) {
    return <div className="visual visual-status">This diagram could not be drawn.</div>;
  }
  return (
    <div
      className="visual diagram"
      role="img"
      aria-label="Diagram of what was described"
      dangerouslySetInnerHTML={{ __html: rendered.svg }}
    />
  );
}
