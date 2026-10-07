import { lazy, StrictMode, Suspense } from "react";
import { createRoot } from "react-dom/client";
import { Overlay } from "./overlay/Overlay";
import { Palette } from "./palette/Palette";
import { createShellBridge } from "./shell/bridge";
import { TopicWindow } from "./topics/TopicWindow";
import "./styles.css";

const root = document.getElementById("root");
if (!root) throw new Error("#root is missing from index.html");

// Development only: every kind of topic window side by side.
const Gallery = lazy(() => import("./topics/Gallery"));

const bridge = createShellBridge();
document.documentElement.dataset.window = bridge.windowKind;

createRoot(root).render(
  <StrictMode>
    {import.meta.env.DEV && bridge.windowKind === "gallery" ? (
      <Suspense fallback={null}>
        <Gallery />
      </Suspense>
    ) : bridge.windowKind === "palette" ? (
      <Palette bridge={bridge} />
    ) : bridge.windowKind === "topic" && bridge.topicId !== null ? (
      <TopicWindow bridge={bridge} topicId={bridge.topicId} />
    ) : (
      <Overlay bridge={bridge} />
    )}
  </StrictMode>,
);
