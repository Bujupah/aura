import { SAMPLE_TOPIC } from "../shell/bridge";
import type { Topic, WindowSize } from "../shell/types";
import { TopicCard } from "./TopicCard";

// Development only (`?window=gallery`): each kind of topic window at its real
// size, so the design can be judged without running a meeting.

const SIZES: Record<WindowSize, readonly [width: number, height: number]> = {
  small: [300, 96],
  medium: [300, 172],
  large: [340, 240],
  tall: [340, 420],
};

function topic(overrides: Partial<Topic> & Pick<Topic, "id" | "title" | "notes">, size: WindowSize): Topic {
  return {
    ...SAMPLE_TOPIC,
    sources: [],
    ...overrides,
    placement: { zone: "topRight", size },
  };
}

const TOPICS: readonly Topic[] = [
  topic({ id: "timeline", title: "Timeline", notes: ["Customer: decision expected before year end."] }, "small"),
  topic(
    {
      id: "alert-noise",
      title: "Alert noise",
      notes: [
        "Customer: far too many alerts from Dynatrace.",
        "Finding the root cause of an incident takes hours.",
        "You promised: send a reference architecture by Friday.",
      ],
    },
    "medium",
  ),
  { ...SAMPLE_TOPIC, placement: { zone: "topRight", size: "large" } },
  topic(
    {
      id: "incident-flow",
      title: "Incident flow",
      notes: [
        "Dynatrace and Splunk both send alerts into ServiceNow.",
        "An engineer looks up the affected server in the CMDB.",
      ],
      diagram:
        'flowchart TD\nA["Dynatrace"] --> C["ServiceNow"]\nB["Splunk"] --> C\nC --> D["Incident"]\nD --> E["CMDB"]\nE -.-> F["Root cause"]',
    },
    "tall",
  ),
  topic(
    {
      id: "environment",
      title: "Current environment",
      notes: ["Customer: runs on AWS and OpenShift."],
      image: { brief: "AWS and OpenShift feeding a CMDB", status: "pending" },
    },
    "tall",
  ),
  topic(
    {
      id: "broken",
      title: "Bad diagram",
      notes: ["A diagram the agent got wrong still leaves the notes readable."],
      diagram: "flowchart TD\nA --> --> ???",
    },
    "tall",
  ),
];

export default function Gallery() {
  return (
    <div className="gallery">
      {TOPICS.map((entry) => {
        const [width, height] = SIZES[entry.placement?.size ?? "medium"];
        return (
          <div key={entry.id} style={{ width, height }}>
            <TopicCard topic={entry} imageUrl={null} onClose={() => undefined} />
          </div>
        );
      })}
    </div>
  );
}
