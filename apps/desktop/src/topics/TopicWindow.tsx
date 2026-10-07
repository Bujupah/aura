import { useEffect, useState } from "react";
import type { ShellBridge } from "../shell/bridge";
import type { Topic } from "../shell/types";
import { TopicCard } from "./TopicCard";

function useTopic(bridge: ShellBridge, topicId: string): Topic | null {
  const [topic, setTopic] = useState<Topic | null>(null);
  useEffect(() => {
    let active = true;
    let sawEvent = false;
    const pick = (topics: readonly Topic[]) => topics.find((candidate) => candidate.id === topicId);
    const unsubscribe = bridge.subscribeMeeting((event) => {
      if (event.type !== "topics") return;
      sawEvent = true;
      // A topic that drops off the board keeps its last notes on screen
      // until the window is closed.
      const next = pick(event.topics);
      if (next) setTopic(next);
    });
    bridge.getTopics().then(
      (topics) => {
        const initial = pick(topics);
        if (active && !sawEvent && initial) setTopic(initial);
      },
      (error: unknown) => console.error("topics_current failed", error),
    );
    return () => {
      active = false;
      unsubscribe();
    };
  }, [bridge, topicId]);
  return topic;
}

/** Fetches the picture once the agent's illustrator reports it ready. */
function useIllustration(bridge: ShellBridge, topic: Topic | null): string | null {
  const [url, setUrl] = useState<string | null>(null);
  const ready = topic?.image?.status === "ready";
  const brief = topic?.image?.brief;
  const topicId = topic?.id;
  useEffect(() => {
    if (!ready || topicId === undefined) {
      setUrl(null);
      return;
    }
    let active = true;
    let created: string | null = null;
    bridge.getTopicImage(topicId).then(
      (next) => {
        created = next;
        if (active) setUrl(next);
        else if (next) URL.revokeObjectURL(next);
      },
      (error: unknown) => console.error("topic_image failed", error),
    );
    return () => {
      active = false;
      if (created) URL.revokeObjectURL(created);
    };
    // A changed brief means a new picture for the same topic.
  }, [bridge, topicId, ready, brief]);
  return url;
}

export function TopicWindow({ bridge, topicId }: { bridge: ShellBridge; topicId: string }) {
  const topic = useTopic(bridge, topicId);
  const imageUrl = useIllustration(bridge, topic);
  if (topic === null) return null;
  return (
    <TopicCard
      topic={topic}
      imageUrl={imageUrl}
      onClose={() => void bridge.dismissTopic(topic.id)}
    />
  );
}
