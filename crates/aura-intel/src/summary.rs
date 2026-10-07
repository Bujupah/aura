use std::collections::HashSet;

use aura_core::summary::{MeetingSummary, SummaryProposal};
use aura_core::topics::Topic;
use aura_core::transcript::Turn;
use serde_json::{json, Value};

use crate::responses::StructuredRequest;
use crate::{IntelError, ResponsesClient};

/// The most turns sent for a summary; a very long meeting keeps its ending.
const MAX_TURNS: usize = 600;

fn schema() -> Value {
    let items = json!({ "type": "array", "items": {
        "type": "object",
        "additionalProperties": false,
        "required": ["text", "turnIds"],
        "properties": {
            "text": { "type": "string" },
            "turnIds": { "type": "array", "items": { "type": "string" } }
        }
    }});
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "headline", "overview", "environment", "painPoints", "requirements",
            "openQuestions", "commitments", "nextStep", "nextStepWhy"
        ],
        "properties": {
            "headline": { "type": "string" },
            "overview": { "type": "string" },
            "environment": items,
            "painPoints": items,
            "requirements": items,
            "openQuestions": items,
            "commitments": items,
            "nextStep": { "type": "string" },
            "nextStepWhy": { "type": "string" }
        }
    })
}

/// Writes up a finished meeting. Items the transcript does not support are
/// removed before the summary is returned.
pub async fn summarize(
    client: &ResponsesClient,
    language: Option<&str>,
    transcript: &[Turn],
    topics: &[Topic],
) -> Result<MeetingSummary, IntelError> {
    let turns = &transcript[transcript.len().saturating_sub(MAX_TURNS)..];
    let input = json!({
        "language": language,
        "topics": topics.iter().map(|topic| json!({ "title": topic.title, "notes": topic.notes })).collect::<Vec<_>>(),
        "turns": turns.iter().map(|turn| json!({
            "id": turn.id, "speaker": turn.speaker, "text": turn.text
        })).collect::<Vec<_>>(),
    })
    .to_string();
    let answer = client
        .structured::<SummaryProposal>(StructuredRequest {
            instructions: crate::prompt_body(include_str!("../../../prompts/post-meeting/summary.md")),
            input: &input,
            schema_name: "meeting_summary",
            schema: schema(),
            web_search: false,
            // The meeting is over: take the time to get it right.
            effort: "medium",
        })
        .await?;
    let known: HashSet<&str> = turns.iter().map(|turn| turn.id.as_str()).collect();
    Ok(MeetingSummary::from_proposal(answer.value, |turn_id| known.contains(turn_id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_schema_requires_every_field_the_parser_reads() {
        let schema = schema();
        let mut example = serde_json::Map::new();
        for field in schema["required"].as_array().unwrap() {
            let name = field.as_str().unwrap();
            let value = match schema["properties"][name]["type"].as_str().unwrap() {
                "array" => json!([{ "text": "x", "turnIds": ["customer-0"] }]),
                _ => json!("x"),
            };
            example.insert(name.to_owned(), value);
        }
        let proposal: SummaryProposal = serde_json::from_value(Value::Object(example)).unwrap();
        assert_eq!(proposal.commitments.len(), 1);
    }
}
