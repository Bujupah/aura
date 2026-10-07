use std::collections::{HashSet, VecDeque};
use std::time::Instant;

use aura_core::topics::{ImageStatus, Topic, TopicBoard, WindowSpec};
use aura_core::transcript::Turn;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::responses::StructuredRequest;
use crate::{Illustrator, ResponsesClient};

/// What the note-taking agent is allowed to reach for in this session.
#[derive(Clone)]
pub struct Abilities {
    pub web_search: bool,
    /// The sub-agent that draws images, if one is available.
    pub illustrator: Option<Illustrator>,
}

/// Where a continued meeting left off. Empty for a new one.
#[derive(Debug, Clone, Default)]
pub struct Earlier {
    pub topics: Vec<Topic>,
    pub turns: Vec<Turn>,
}

/// What the agent's work produces.
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    /// The complete current set of topics and their arrangement.
    Topics(Vec<Topic>),
    /// A finished illustration (PNG). Always sent before the `Topics` update
    /// that marks it ready.
    Image { topic_id: String, png: Vec<u8> },
}

/// Something the seller did that the agent must respect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Control {
    /// The seller closed this topic's window: it is gone for good.
    Dismiss { topic_id: String },
}

/// How many closed topics the agent is reminded of.
const REMEMBERED_DISMISSALS: usize = 12;

struct Illustration {
    topic_id: String,
    brief: String,
    result: Result<Vec<u8>, crate::IntelError>,
}

/// Earlier turns sent along so short replies ("yes", "the second one") make
/// sense. Context only.
const CONTEXT_TURNS: usize = 6;

/// The agent's answer: the complete arrangement of windows it wants.
#[derive(Deserialize)]
struct Arrangement {
    windows: Vec<WindowSpec>,
}

fn schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["windows"],
        "properties": { "windows": { "type": "array", "items": {
            "type": "object",
            "additionalProperties": false,
            "required": ["id", "keep", "title", "notes", "turnIds", "sources", "diagram", "imageBrief", "zone", "size"],
            "properties": {
                "id": { "type": "string" },
                "keep": { "type": "boolean" },
                "title": { "type": "string" },
                "notes": { "type": "array", "items": { "type": "string" } },
                "turnIds": { "type": "array", "items": { "type": "string" } },
                "sources": { "type": "array", "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["title", "url"],
                    "properties": { "title": { "type": "string" }, "url": { "type": "string" } }
                }},
                "diagram": { "type": "string" },
                "imageBrief": { "type": "string" },
                "zone": { "type": "string", "enum": ["topLeft", "topRight", "bottomLeft", "bottomRight"] },
                "size": { "type": "string", "enum": ["small", "medium", "large", "tall"] }
            }
        }}}
    })
}

fn instructions() -> &'static str {
    crate::prompt_body(include_str!("../../../prompts/extraction/topics.md"))
}

fn request(
    board: &TopicBoard,
    dismissed: &[String],
    context: &VecDeque<Turn>,
    new_turns: &[Turn],
    abilities: &Abilities,
) -> String {
    let turn = |turn: &Turn| json!({ "id": turn.id, "speaker": turn.speaker, "text": turn.text });
    let (shown, put_away): (Vec<&Topic>, Vec<&Topic>) =
        board.topics().iter().partition(|topic| topic.placement.is_some());
    json!({
        "windows": shown.iter().map(|topic| json!({
            "id": topic.id,
            "title": topic.title,
            "notes": topic.notes,
            // Already cited; offered so windows can be merged or split.
            "turnIds": topic.source_turn_ids,
            "sources": topic.sources,
            "diagram": topic.diagram.as_deref().unwrap_or_default(),
            "imageBrief": topic.image.as_ref().map(|image| image.brief.as_str()).unwrap_or_default(),
            "imageStatus": topic.image.as_ref().map(|image| image.status),
            "zone": topic.placement.map(|placement| placement.zone),
            "size": topic.placement.map(|placement| placement.size),
        })).collect::<Vec<_>>(),
        "putAway": put_away.iter().map(|topic| json!({
            "id": topic.id, "title": topic.title, "notes": topic.notes,
            "turnIds": topic.source_turn_ids, "sources": topic.sources,
        })).collect::<Vec<_>>(),
        "closedBySeller": dismissed,
        "recentTurns": context.iter().map(turn).collect::<Vec<_>>(),
        "newTurns": new_turns.iter().map(turn).collect::<Vec<_>>(),
        "webSearch": abilities.web_search,
        "imageAgent": abilities.illustrator.is_some(),
    })
    .to_string()
}

/// Hands the agent's arrangement to the board, which decides what holds up.
fn accept(
    board: &mut TopicBoard,
    arrangement: Arrangement,
    new_turns: &[Turn],
    retrieved_urls: &[String],
    now_ms: u64,
) -> bool {
    let offered: HashSet<&str> = new_turns.iter().map(|turn| turn.id.as_str()).collect();
    let retrieved: HashSet<&str> = retrieved_urls.iter().map(|url| page(url)).collect();
    let (changed, rejections) = board.arrange(
        arrangement.windows,
        |turn_id| offered.contains(turn_id),
        |url| retrieved.contains(page(url)),
        now_ms,
    );
    for (id, rejection) in rejections {
        tracing::warn!(event = "window_spec_rejected", %id, ?rejection);
    }
    changed
}

/// A URL without query, fragment or trailing slash: search results and
/// citations differ in tracking parameters.
fn page(url: &str) -> &str {
    let end = url.find(['?', '#']).unwrap_or(url.len());
    url[..end].trim().trim_end_matches('/')
}

/// Consumes finalized turns and calls `publish` whenever the notes, their
/// arrangement or an illustration change. Runs until `turns` closes.
///
/// One arrangement request is in flight at a time; turns that arrive
/// meanwhile are processed together in the next one. If a request fails its
/// turns are retried with the next batch, so a transient error loses nothing.
/// Illustrations are drawn in the background and never hold up the notes.
pub async fn track_topics(
    client: ResponsesClient,
    abilities: Abilities,
    earlier: Earlier,
    mut turns: mpsc::UnboundedReceiver<Turn>,
    mut control: mpsc::UnboundedReceiver<Control>,
    publish: impl Fn(Update),
) {
    let started = Instant::now();
    // A continued meeting's notes keep their place on its timeline.
    let base_ms = earlier.topics.iter().map(|topic| topic.updated_at_ms).max().unwrap_or(0);
    let mut board = TopicBoard::restore(earlier.topics);
    let mut context: VecDeque<Turn> =
        earlier.turns.into_iter().rev().take(CONTEXT_TURNS).rev().collect();
    let mut pending: Vec<Turn> = Vec::new();
    let (drawn, mut illustrations) = mpsc::unbounded_channel::<Illustration>();
    // Requests already handed to the illustrator: (topic id, brief).
    let mut commissioned: HashSet<(String, String)> = HashSet::new();
    // Titles of topics the seller closed, most recent last.
    let mut dismissed: Vec<String> = Vec::new();

    loop {
        tokio::select! {
            turn = turns.recv() => {
                let Some(turn) = turn else { return };
                pending.push(turn);
                while let Ok(turn) = turns.try_recv() {
                    pending.push(turn);
                }
            }
            Some(Control::Dismiss { topic_id }) = control.recv() => {
                if let Some(title) = board.remove(&topic_id) {
                    tracing::info!(event = "topic_dismissed", topic = %topic_id);
                    dismissed.push(title);
                    if dismissed.len() > REMEMBERED_DISMISSALS {
                        dismissed.remove(0);
                    }
                    publish(Update::Topics(board.topics().to_vec()));
                }
                continue;
            }
            Some(illustration) = illustrations.recv() => {
                let status = match illustration.result {
                    Ok(png) => {
                        publish(Update::Image { topic_id: illustration.topic_id.clone(), png });
                        ImageStatus::Ready
                    }
                    Err(error) => {
                        tracing::warn!(event = "illustration_failed", topic = %illustration.topic_id, %error);
                        ImageStatus::Failed
                    }
                };
                if board.set_image_status(&illustration.topic_id, &illustration.brief, status) {
                    publish(Update::Topics(board.topics().to_vec()));
                }
                continue;
            }
        }

        let requested = Instant::now();
        let input = request(&board, &dismissed, &context, &pending, &abilities);
        let result = client
            .structured::<Arrangement>(StructuredRequest {
                instructions: instructions(),
                input: &input,
                schema_name: "arrangement",
                schema: schema(),
                web_search: abilities.web_search.into(),
                effort: "low",
            })
            .await;
        match result {
            Ok(answer) => {
                let now_ms = base_ms + started.elapsed().as_millis() as u64;
                let changed = accept(&mut board, answer.value, &pending, &answer.retrieved_urls, now_ms);
                tracing::info!(
                    event = "windows_arranged",
                    turns = pending.len(),
                    windows = board.topics().iter().filter(|topic| topic.placement.is_some()).count(),
                    put_away = board.topics().iter().filter(|topic| topic.placement.is_none()).count(),
                    diagrams = board.topics().iter().filter(|topic| topic.diagram.is_some()).count(),
                    searched = !answer.retrieved_urls.is_empty(),
                    changed,
                    latency_ms = requested.elapsed().as_millis() as u64
                );
                commission(&mut board, &abilities, &mut commissioned, &drawn);
                if changed {
                    publish(Update::Topics(board.topics().to_vec()));
                }
                for turn in pending.drain(..) {
                    if context.len() == CONTEXT_TURNS {
                        context.pop_front();
                    }
                    context.push_back(turn);
                }
            }
            Err(error) => tracing::warn!(event = "arrangement_failed", %error, turns = pending.len()),
        }
    }
}

/// Starts the illustrator on every image request it has not seen yet. With
/// no illustrator, requests are marked failed so no window waits forever.
fn commission(
    board: &mut TopicBoard,
    abilities: &Abilities,
    commissioned: &mut HashSet<(String, String)>,
    drawn: &mpsc::UnboundedSender<Illustration>,
) {
    let waiting: Vec<Topic> = board
        .topics()
        .iter()
        .filter(|topic| topic.image.as_ref().is_some_and(|image| image.status == ImageStatus::Pending))
        .cloned()
        .collect();
    for topic in waiting {
        let Some(brief) = topic.image.as_ref().map(|image| image.brief.clone()) else {
            continue;
        };
        if !commissioned.insert((topic.id.clone(), brief.clone())) {
            continue;
        }
        match &abilities.illustrator {
            Some(illustrator) => {
                let (illustrator, drawn) = (illustrator.clone(), drawn.clone());
                tracing::info!(event = "illustration_commissioned", topic = %topic.id);
                tokio::spawn(async move {
                    let requested = Instant::now();
                    let result = illustrator.illustrate(&brief, &topic).await;
                    tracing::info!(
                        event = "illustration_finished",
                        topic = %topic.id,
                        ok = result.is_ok(),
                        latency_ms = requested.elapsed().as_millis() as u64
                    );
                    let _ = drawn.send(Illustration { topic_id: topic.id, brief, result });
                });
            }
            None => {
                board.set_image_status(&topic.id, &brief, ImageStatus::Failed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::topics::{Size, Zone};
    use aura_core::transcript::Speaker;

    fn turn(id: &str, text: &str) -> Turn {
        Turn {
            id: id.into(),
            speaker: Speaker::Customer,
            text: text.into(),
            start_ms: 0,
            end_ms: 1,
            is_final: true,
            translation: None,
        }
    }

    fn arrangement(raw: Value) -> Arrangement {
        serde_json::from_value(raw).unwrap()
    }

    fn window(id: &str, notes: Value, turn_ids: Value, sources: Value) -> Value {
        json!({ "id": id, "keep": false, "title": id, "notes": notes, "turnIds": turn_ids, "sources": sources,
                "diagram": "", "imageBrief": "", "zone": "topRight", "size": "medium" })
    }

    #[test]
    fn the_prompt_is_loaded_without_its_front_matter() {
        assert!(instructions().starts_with("You run a set of small private note windows"));
        assert!(!instructions().contains("version:"));
    }

    #[test]
    fn the_schema_and_the_parser_agree_on_every_zone_and_size() {
        let schema = schema();
        let item = &schema["properties"]["windows"]["items"]["properties"];
        for zone in item["zone"]["enum"].as_array().unwrap() {
            for size in item["size"]["enum"].as_array().unwrap() {
                let parsed = arrangement(json!({ "windows": [{
                    "id": "a", "keep": false, "title": "A", "notes": ["n"], "turnIds": [], "sources": [],
                    "diagram": "", "imageBrief": "", "zone": zone, "size": size
                }]}));
                assert_eq!(parsed.windows.len(), 1);
            }
        }
        let parsed = arrangement(json!({ "windows": [window("a", json!(["n"]), json!([]), json!([]))] }));
        assert_eq!((parsed.windows[0].zone, parsed.windows[0].size), (Zone::TopRight, Size::Medium));
    }

    #[test]
    fn the_request_shows_the_agent_its_current_arrangement() {
        let mut board = TopicBoard::default();
        let first = [turn("customer-0", "Our CMDB is stale.")];
        accept(
            &mut board,
            arrangement(json!({ "windows": [window("cmdb", json!(["Stale"]), json!(["customer-0"]), json!([]))] })),
            &first,
            &[],
            0,
        );
        let mut context = VecDeque::new();
        context.push_back(first[0].clone());
        let abilities = Abilities { web_search: true, illustrator: None };
        let request: Value =
            serde_json::from_str(&request(
                &board,
                &["Small talk".to_owned()],
                &context,
                &[turn("customer-1", "It is slow.")],
                &abilities,
            ))
            .unwrap();
        assert_eq!(request["closedBySeller"], json!(["Small talk"]));
        assert_eq!(request["windows"][0]["id"], "cmdb");
        assert_eq!(request["windows"][0]["zone"], "topRight");
        assert_eq!(request["windows"][0]["turnIds"], json!(["customer-0"]));
        assert_eq!(request["putAway"], json!([]));
        assert_eq!(request["recentTurns"][0]["id"], "customer-0");
        assert_eq!(request["newTurns"][0], json!({"id": "customer-1", "speaker": "customer", "text": "It is slow."}));
        assert_eq!(request["webSearch"], true);
        assert_eq!(request["imageAgent"], false);
        assert_eq!(request["windows"][0]["diagram"], "");
    }

    #[test]
    fn new_content_must_cite_a_turn_from_the_batch_it_was_shown() {
        let mut board = TopicBoard::default();
        let changed = accept(
            &mut board,
            arrangement(json!({ "windows": [
                window("cmdb", json!(["Stale"]), json!(["customer-4"]), json!([])),
                window("budget", json!(["2M approved"]), json!(["customer-0", "customer-99"]), json!([]))
            ]})),
            &[turn("customer-4", "Our CMDB is stale.")],
            &[],
            10,
        );
        assert!(changed);
        assert_eq!(board.topics().len(), 1);
        assert_eq!(board.topics()[0].id, "cmdb");
    }

    #[test]
    fn a_web_note_survives_only_if_its_page_came_back_from_a_search() {
        let spec = |url: &str| {
            arrangement(json!({ "windows": [window(
                "ocp",
                json!(["Open: OpenShift support?", "Web: An OpenShift provider is documented."]),
                json!(["customer-0"]),
                json!([{ "title": "Docs", "url": url }])
            )]}))
        };
        let turns = [turn("customer-0", "Does it support OpenShift?")];
        let retrieved = ["https://docs.example.com/openshift/".to_string()];

        let mut board = TopicBoard::default();
        accept(&mut board, spec("https://docs.example.com/openshift?utm_source=openai"), &turns, &retrieved, 0);
        assert_eq!(board.topics()[0].notes.len(), 2);
        assert_eq!(board.topics()[0].sources.len(), 1);

        let mut board = TopicBoard::default();
        accept(&mut board, spec("https://invented.example.com/page"), &turns, &retrieved, 0);
        assert_eq!(board.topics()[0].notes, ["Open: OpenShift support?"]);
    }

    #[test]
    fn nothing_accepted_means_nothing_published() {
        let mut board = TopicBoard::default();
        assert!(!accept(&mut board, arrangement(json!({ "windows": [] })), &[turn("customer-0", "Hi")], &[], 0));
    }

    #[test]
    fn an_image_request_without_an_illustrator_fails_instead_of_waiting_forever() {
        let mut board = TopicBoard::default();
        let mut spec = window("env", json!(["Runs AWS"]), json!(["customer-0"]), json!([]));
        spec["imageBrief"] = json!("AWS feeding a CMDB");
        accept(&mut board, arrangement(json!({ "windows": [spec] })), &[turn("customer-0", "We run AWS.")], &[], 0);
        assert_eq!(board.topics()[0].image.as_ref().unwrap().status, ImageStatus::Pending);

        let (drawn, _results) = mpsc::unbounded_channel();
        let abilities = Abilities { web_search: false, illustrator: None };
        commission(&mut board, &abilities, &mut HashSet::new(), &drawn);
        assert_eq!(board.topics()[0].image.as_ref().unwrap().status, ImageStatus::Failed);
    }
}
