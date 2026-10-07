use std::collections::HashSet;
use std::time::Instant;

use aura_core::advice::{self, Advice, AdviceProposal, Gaps};
use aura_core::topics::Topic;
use aura_core::transcript::Turn;
use serde_json::{json, Value};
use tokio::sync::{mpsc, watch};

use crate::responses::StructuredRequest;
use crate::ResponsesClient;

/// Turns of context for a live suggestion: enough to follow the thread,
/// short enough to answer quickly.
const RECENT_TURNS: usize = 10;
/// The most of a meeting sent when the seller asks what is missing.
const REVIEW_TURNS: usize = 200;

/// Something the seller asked the advisor for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdvisorRequest {
    WhatsMissing,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AdvisorUpdate {
    /// The move to show, or `None` to show nothing.
    Advice(Option<Advice>),
    /// The answer to "what are we missing?", or `None` if it could not be
    /// worked out.
    Gaps(Option<Gaps>),
}

fn advice_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["kind", "text", "why", "turnIds"],
        "properties": {
            "kind": { "type": "string", "enum": ["ask", "say", "caution", "none"] },
            "text": { "type": "string" },
            "why": { "type": "string" },
            "turnIds": { "type": "array", "items": { "type": "string" } }
        }
    })
}

fn gaps_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["understood", "missing", "priority"],
        "properties": {
            "understood": { "type": "string" },
            "missing": { "type": "array", "items": { "type": "string" } },
            "priority": { "type": "string" }
        }
    })
}

fn turns_json(turns: &[Turn]) -> Value {
    turns
        .iter()
        .map(|turn| json!({ "id": turn.id, "speaker": turn.speaker, "text": turn.text }))
        .collect()
}

fn topics_json(topics: &[Topic]) -> Value {
    topics
        .iter()
        .map(|topic| json!({ "title": topic.title, "notes": topic.notes }))
        .collect()
}

fn tail<T>(items: &[T], count: usize) -> &[T] {
    &items[items.len().saturating_sub(count)..]
}

/// Watches the meeting and publishes the one next move for the seller, and
/// answers "what are we missing?" on request. Runs until `turns` closes.
///
/// `fast` answers while the conversation is moving; `deep` takes the time to
/// review the whole meeting when the seller asks for it.
pub async fn track_advice(
    fast: ResponsesClient,
    deep: ResponsesClient,
    language: Option<String>,
    mut turns: mpsc::UnboundedReceiver<Turn>,
    topics: watch::Receiver<Vec<Topic>>,
    mut requests: mpsc::UnboundedReceiver<AdvisorRequest>,
    publish: impl Fn(AdvisorUpdate),
) {
    let started = Instant::now();
    let mut transcript: Vec<Turn> = Vec::new();
    let mut known: HashSet<String> = HashSet::new();
    let mut current: Option<Advice> = None;

    loop {
        tokio::select! {
            turn = turns.recv() => {
                let Some(turn) = turn else { return };
                let mut heard = vec![turn];
                while let Ok(turn) = turns.try_recv() {
                    heard.push(turn);
                }
                for turn in heard {
                    known.insert(turn.id.clone());
                    transcript.push(turn);
                }
            }
            Some(AdvisorRequest::WhatsMissing) = requests.recv() => {
                let input = json!({
                    "language": language,
                    "topics": topics_json(&topics.borrow()),
                    "turns": turns_json(tail(&transcript, REVIEW_TURNS)),
                })
                .to_string();
                let requested = Instant::now();
                let result = deep
                    .structured::<Gaps>(StructuredRequest {
                        instructions: crate::prompt_body(include_str!("../../../prompts/reasoning/whats-missing.md")),
                        input: &input,
                        schema_name: "gaps",
                        schema: gaps_schema(),
                        web_search: false,
                        effort: "low",
                    })
                    .await;
                tracing::info!(
                    event = "gaps_reviewed",
                    ok = result.is_ok(),
                    latency_ms = requested.elapsed().as_millis() as u64
                );
                publish(AdvisorUpdate::Gaps(match result {
                    Ok(answer) => Some(answer.value.tidy()),
                    Err(error) => {
                        tracing::warn!(event = "gaps_failed", %error);
                        None
                    }
                }));
                continue;
            }
        }

        let input = json!({
            "language": language,
            "topics": topics_json(&topics.borrow()),
            "recentTurns": turns_json(tail(&transcript, RECENT_TURNS)),
            "current": current.as_ref().map(|advice| json!({
                "kind": advice.kind, "text": advice.text, "why": advice.why
            })),
        })
        .to_string();
        let requested = Instant::now();
        let result = fast
            .structured::<AdviceProposal>(StructuredRequest {
                instructions: crate::prompt_body(include_str!("../../../prompts/reasoning/next-move.md")),
                input: &input,
                schema_name: "next_move",
                schema: advice_schema(),
                web_search: false,
                effort: "low",
            })
            .await;
        match result {
            Ok(answer) => {
                let now_ms = started.elapsed().as_millis() as u64;
                let next = advice::accept(answer.value, |turn_id| known.contains(turn_id), now_ms);
                tracing::info!(
                    event = "advice_considered",
                    kind = ?next.as_ref().map(|advice| advice.kind),
                    latency_ms = requested.elapsed().as_millis() as u64
                );
                // The same move restated is not news; leave what is on
                // screen, with its original time, alone.
                let unchanged = match (&current, &next) {
                    (Some(shown), Some(proposed)) => shown.kind == proposed.kind && shown.text == proposed.text,
                    (None, None) => true,
                    _ => false,
                };
                if !unchanged {
                    current = next;
                    publish(AdvisorUpdate::Advice(current.clone()));
                }
            }
            // A failed request leaves the current suggestion in place.
            Err(error) => tracing::warn!(event = "advice_failed", %error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::advice::ProposedKind;

    #[test]
    fn the_schema_and_the_parser_agree_on_every_kind() {
        for kind in advice_schema()["properties"]["kind"]["enum"].as_array().unwrap() {
            let proposal: AdviceProposal =
                serde_json::from_value(json!({ "kind": kind, "text": "", "why": "", "turnIds": [] })).unwrap();
            assert_eq!(proposal.kind == ProposedKind::None, kind == "none");
        }
    }

    #[test]
    fn gaps_parse_from_the_schema_shape() {
        let gaps: Gaps = serde_json::from_value(json!({
            "understood": "The alerting problem.", "missing": ["Who owns the CMDB?"], "priority": "Ask about ownership."
        }))
        .unwrap();
        assert_eq!(gaps.missing, ["Who owns the CMDB?"]);
    }

    #[test]
    fn only_the_most_recent_turns_are_sent_for_a_live_suggestion() {
        let turns: Vec<u32> = (0..25).collect();
        assert_eq!(tail(&turns, RECENT_TURNS), &turns[15..]);
        assert_eq!(tail(&turns[..3], RECENT_TURNS), &turns[..3]);
    }
}
