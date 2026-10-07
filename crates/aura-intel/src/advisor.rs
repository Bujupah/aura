use std::collections::HashSet;
use std::time::Instant;

use aura_core::advice::{self, Advice, AdviceProposal, Answer, AnswerProposal, Gaps};
use aura_core::topics::Topic;
use aura_core::transcript::Turn;
use serde_json::{json, Value};
use tokio::sync::{mpsc, watch};

use crate::responses::{StructuredRequest, WebSearch};
use crate::ResponsesClient;

/// Turns of context for a live suggestion: enough to follow the thread,
/// short enough to answer quickly.
const RECENT_TURNS: usize = 10;
/// The most of a meeting sent when the seller asks what is missing.
const REVIEW_TURNS: usize = 200;

/// What the advisor works with for the length of a meeting.
pub struct AdvisorSetup {
    /// Answers while the conversation is moving.
    pub fast: ResponsesClient,
    /// Takes the time to review the whole meeting when asked.
    pub deep: ResponsesClient,
    /// The language to write in, or `None` to match the seller's turns.
    pub language: Option<String>,
    /// The turns of a meeting being continued; empty for a new one.
    pub earlier: Vec<Turn>,
}

/// One of the things the seller can ask for from the command palette.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Ask {
    AskNext,
    Explain,
    Promised,
    Summarize,
    Environment,
    Architecture,
    Think,
    Demo,
    CanHelix,
    Verify,
    Answer,
    SearchDocs,
    Compare,
    /// The seller's own question.
    Question,
}

/// The vendor's own sites: the only places an answer about its products can
/// be verified from until approved internal material is available.
const VENDOR_DOMAINS: &[&str] = &["bmc.com"];

impl Ask {
    fn web_search(&self) -> WebSearch {
        match self {
            Self::CanHelix | Self::Verify | Self::Answer | Self::SearchDocs => WebSearch::Domains(VENDOR_DOMAINS),
            Self::Compare | Self::Question => WebSearch::Open,
            _ => WebSearch::Off,
        }
    }

    /// Hosts whose pages can make an answer "verified".
    fn trusted_hosts(&self) -> &'static [&'static str] {
        match self.web_search() {
            WebSearch::Off => &[],
            // An open search can still land on the vendor's documentation.
            WebSearch::Open | WebSearch::Domains(_) => VENDOR_DOMAINS,
        }
    }

    /// Quick recaps use the fast model; anything that searches or reasons
    /// about the whole meeting uses the stronger one.
    fn is_quick(&self) -> bool {
        matches!(self, Self::AskNext | Self::Promised | Self::Explain)
    }
}

/// Something the seller asked the advisor for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdvisorRequest {
    WhatsMissing,
    Ask { ask: Ask, question: Option<String> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum AdvisorUpdate {
    /// The move to show, or `None` to show nothing.
    Advice(Option<Advice>),
    /// The answer to "what are we missing?", or `None` if it could not be
    /// worked out.
    Gaps(Option<Gaps>),
    /// The reply to a palette request, or `None` if it could not be produced.
    Answer(Option<Answer>),
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

fn answer_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["title", "summary", "points", "sayThis", "diagram", "verification", "sources"],
        "properties": {
            "title": { "type": "string" },
            "summary": { "type": "string" },
            "points": { "type": "array", "items": { "type": "string" } },
            "sayThis": { "type": "string" },
            "diagram": { "type": "string" },
            "verification": { "type": "string", "enum": ["verified", "unverified", "notApplicable"] },
            "sources": { "type": "array", "items": {
                "type": "object",
                "additionalProperties": false,
                "required": ["title", "url"],
                "properties": { "title": { "type": "string" }, "url": { "type": "string" } }
            }}
        }
    })
}

/// A URL without query, fragment or trailing slash, for comparing a cited
/// page with what a search returned.
fn page(url: &str) -> &str {
    let end = url.find(['?', '#']).unwrap_or(url.len());
    url[..end].trim().trim_end_matches('/')
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
pub async fn track_advice(
    setup: AdvisorSetup,
    mut turns: mpsc::UnboundedReceiver<Turn>,
    topics: watch::Receiver<Vec<Topic>>,
    mut requests: mpsc::UnboundedReceiver<AdvisorRequest>,
    publish: impl Fn(AdvisorUpdate),
) {
    let AdvisorSetup { fast, deep, language, earlier } = setup;
    let started = Instant::now();
    // A continued meeting starts with everything said before.
    let mut known: HashSet<String> = earlier.iter().map(|turn| turn.id.clone()).collect();
    let mut transcript: Vec<Turn> = earlier;
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
            Some(request) = requests.recv() => {
                let AdvisorRequest::Ask { ask, question } = request else {
                    // Copied out first: the lock must not be held across the request.
                    let notes = topics.borrow().clone();
                    review_gaps(&deep, language.as_deref(), &notes, &transcript, &publish).await;
                    continue;
                };
                let web_search = ask.web_search();
                let input = json!({
                    "command": ask,
                    "question": question,
                    "language": language,
                    "webSearch": match web_search {
                        WebSearch::Off => "off",
                        WebSearch::Open => "open",
                        WebSearch::Domains(_) => "vendorDocs",
                    },
                    "topics": topics_json(&topics.borrow()),
                    "turns": turns_json(tail(&transcript, REVIEW_TURNS)),
                })
                .to_string();
                let requested = Instant::now();
                let result = if ask.is_quick() { &fast } else { &deep }
                    .structured::<AnswerProposal>(StructuredRequest {
                        instructions: crate::prompt_body(include_str!("../../../prompts/reasoning/on-request.md")),
                        input: &input,
                        schema_name: "answer",
                        schema: answer_schema(),
                        web_search,
                        effort: "low",
                    })
                    .await;
                tracing::info!(
                    event = "request_answered",
                    command = ?ask,
                    ok = result.is_ok(),
                    searched = result.as_ref().is_ok_and(|answer| !answer.retrieved_urls.is_empty()),
                    latency_ms = requested.elapsed().as_millis() as u64
                );
                publish(AdvisorUpdate::Answer(match result {
                    Ok(answer) => {
                        let retrieved: HashSet<&str> = answer.retrieved_urls.iter().map(|url| page(url)).collect();
                        Some(advice::accept_answer(
                            answer.value,
                            |url| retrieved.contains(page(url)),
                            ask.trusted_hosts(),
                        ))
                    }
                    Err(error) => {
                        tracing::warn!(event = "request_failed", %error);
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
                web_search: WebSearch::Off,
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

async fn review_gaps(
    deep: &ResponsesClient,
    language: Option<&str>,
    topics: &[Topic],
    transcript: &[Turn],
    publish: &impl Fn(AdvisorUpdate),
) {
    let input = json!({
        "language": language,
        "topics": topics_json(topics),
        "turns": turns_json(tail(transcript, REVIEW_TURNS)),
    })
    .to_string();
    let requested = Instant::now();
    let result = deep
        .structured::<Gaps>(StructuredRequest {
            instructions: crate::prompt_body(include_str!("../../../prompts/reasoning/whats-missing.md")),
            input: &input,
            schema_name: "gaps",
            schema: gaps_schema(),
            web_search: WebSearch::Off,
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

    #[test]
    fn the_answer_schema_and_the_parser_agree() {
        for verification in answer_schema()["properties"]["verification"]["enum"].as_array().unwrap() {
            let proposal: AnswerProposal = serde_json::from_value(json!({
                "title": "t", "summary": "s", "points": [], "sayThis": "", "diagram": "",
                "verification": verification, "sources": [{ "title": "Docs", "url": "https://docs.bmc.com/x" }]
            }))
            .unwrap();
            assert_eq!(proposal.sources.len(), 1);
        }
    }

    #[test]
    fn product_questions_search_only_the_vendors_sites() {
        for ask in [Ask::CanHelix, Ask::Verify, Ask::Answer, Ask::SearchDocs] {
            assert_eq!(ask.web_search(), WebSearch::Domains(VENDOR_DOMAINS), "{ask:?}");
            assert_eq!(ask.trusted_hosts(), VENDOR_DOMAINS);
        }
    }

    #[test]
    fn recaps_of_the_meeting_never_search_and_can_never_be_verified() {
        for ask in [Ask::AskNext, Ask::Explain, Ask::Promised, Ask::Summarize, Ask::Environment, Ask::Architecture, Ask::Think, Ask::Demo] {
            assert_eq!(ask.web_search(), WebSearch::Off, "{ask:?}");
            assert!(ask.trusted_hosts().is_empty());
        }
    }

    #[test]
    fn commands_use_the_names_the_prompt_documents() {
        let prompt = include_str!("../../../prompts/reasoning/on-request.md");
        for ask in [
            Ask::AskNext, Ask::Explain, Ask::Promised, Ask::Summarize, Ask::Environment, Ask::Architecture,
            Ask::Think, Ask::Demo, Ask::CanHelix, Ask::Verify, Ask::Answer, Ask::SearchDocs, Ask::Compare, Ask::Question,
        ] {
            let name = serde_json::to_value(&ask).unwrap();
            assert!(prompt.contains(&format!("- `{}`", name.as_str().unwrap())), "{name} is not described in the prompt");
        }
    }
}
