//! Topic notes: what has been said in the meeting, grouped by subject, and
//! how the windows showing them are arranged.
//!
//! The note-taking agent owns the arrangement: after every batch of turns it
//! states the complete set of windows — which exist, in what order, where and
//! how large. This module applies that, with two limits the agent cannot
//! override: a note must trace back to something a person said, and placement
//! is expressed in a fixed vocabulary that always fits on screen. Anything
//! the agent looked up on the web is kept visibly apart from what was said,
//! and only survives if it names a page a search really returned.

use serde::{Deserialize, Serialize};

/// The corner a window is stacked from. The middle of the screen belongs to
/// the meeting and is not offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Zone {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Size {
    Small,
    Medium,
    Large,
    /// Room for a diagram or an image above the notes.
    Tall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Placement {
    pub zone: Zone,
    pub size: Size,
}

/// A web page the agent relied on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImageStatus {
    /// The illustration sub-agent is working on it.
    Pending,
    Ready,
    Failed,
}

/// An illustration the agent asked a sub-agent to draw. The picture itself
/// is held outside the board; this records what was asked and how it went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopicImage {
    pub brief: String,
    pub status: ImageStatus,
}

/// Notes that state something found on the web, rather than something said
/// in the meeting, start with this label.
pub const WEB_NOTE_PREFIX: &str = "web:";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Topic {
    pub id: String,
    pub title: String,
    pub notes: Vec<String>,
    /// Every turn that has contributed to this topic.
    pub source_turn_ids: Vec<String>,
    /// Pages behind this topic's `Web:` notes.
    pub sources: Vec<Source>,
    /// Mermaid source for a diagram of what was said, if the agent drew one.
    pub diagram: Option<String>,
    pub image: Option<TopicImage>,
    pub updated_at_ms: u64,
    /// `None` when the agent has put the topic away: its notes are kept, but
    /// it has no window.
    pub placement: Option<Placement>,
}

/// One window in the arrangement the agent asks for. Untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowSpec {
    pub id: String,
    pub title: String,
    pub notes: Vec<String>,
    /// Turns supporting the notes. Required whenever the content is new or
    /// has changed.
    pub turn_ids: Vec<String>,
    /// Pages supporting any `Web:` notes.
    #[serde(default)]
    pub sources: Vec<Source>,
    /// Mermaid source, or empty for no diagram.
    #[serde(default)]
    pub diagram: String,
    /// What the illustration sub-agent should draw, or empty for no image.
    /// Ignored when there is a diagram.
    #[serde(default)]
    pub image_brief: String,
    pub zone: Zone,
    pub size: Size,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    /// The id had no usable characters.
    NoId,
    NoTitle,
    NoNotes,
    /// New or changed content cited no turn the agent could have read.
    NoEvidence,
    /// More windows than can be shown.
    TooMany,
    Duplicate,
}

pub const MAX_TOPICS: usize = 16;
/// Beyond this the screen stops being glanceable, whatever the arrangement.
pub const MAX_WINDOWS: usize = 8;
pub const MAX_NOTES: usize = 5;
const MAX_ID_CHARS: usize = 48;
const MAX_TITLE_CHARS: usize = 48;
const MAX_NOTE_CHARS: usize = 160;
const MAX_SOURCES: usize = 3;
const MAX_SOURCE_TITLE_CHARS: usize = 80;
const MAX_DIAGRAM_CHARS: usize = 1_500;
const MAX_IMAGE_BRIEF_CHARS: usize = 300;

#[derive(Debug, Default)]
pub struct TopicBoard {
    /// Topics with a window first, in the agent's order; then the rest.
    topics: Vec<Topic>,
}

impl TopicBoard {
    pub fn topics(&self) -> &[Topic] {
        &self.topics
    }

    /// Replaces the arrangement with `windows`, in that order. Topics left
    /// out lose their window but keep their notes.
    ///
    /// `turn_is_new` says whether a turn id is one the agent was just shown.
    /// Content may cite those, or turns already cited somewhere on the board
    /// (so topics can be merged or split) — never an id from nowhere.
    /// `url_was_retrieved` says whether a web search in this same step
    /// actually returned a URL; a `Web:` note with no such page is dropped.
    ///
    /// Returns whether anything changed, and the specs that were refused. A
    /// refused content change still lets the agent move an existing window.
    pub fn arrange(
        &mut self,
        windows: Vec<WindowSpec>,
        turn_is_new: impl Fn(&str) -> bool,
        url_was_retrieved: impl Fn(&str) -> bool,
        now_ms: u64,
    ) -> (bool, Vec<(String, Rejection)>) {
        let before = self.topics.clone();
        let known_pages: Vec<String> = self
            .topics
            .iter()
            .flat_map(|topic| topic.sources.iter().map(|source| comparable(&source.url).to_owned()))
            .collect();
        let already_cited: Vec<String> = self
            .topics
            .iter()
            .flat_map(|topic| topic.source_turn_ids.iter().cloned())
            .collect();
        let mut rejections = Vec::new();
        let mut placed: Vec<Topic> = Vec::new();

        for spec in windows {
            let id = slug(&spec.id);
            let refused = if id.is_empty() {
                Some(Rejection::NoId)
            } else if placed.iter().any(|topic| topic.id == id) {
                Some(Rejection::Duplicate)
            } else if placed.len() >= MAX_WINDOWS {
                Some(Rejection::TooMany)
            } else {
                None
            };
            if let Some(rejection) = refused {
                rejections.push((spec.id, rejection));
                continue;
            }

            let placement = Some(Placement {
                zone: spec.zone,
                size: spec.size,
            });
            let existing = self.topics.iter().position(|topic| topic.id == id);
            let content = content_of(
                &spec,
                |turn_id| turn_is_new(turn_id) || already_cited.iter().any(|cited| cited == turn_id),
                |url| url_was_retrieved(url) || known_pages.iter().any(|page| page == comparable(url)),
            );

            match (existing.map(|index| self.topics.remove(index)), content) {
                (Some(mut topic), Ok(content)) => {
                    let same_brief = topic.image.as_ref().map(|image| &image.brief) == content.image_brief.as_ref();
                    if topic.title != content.title
                        || topic.notes != content.notes
                        || topic.sources != content.sources
                        || topic.diagram != content.diagram
                        || !same_brief
                    {
                        topic.title = content.title;
                        topic.notes = content.notes;
                        topic.sources = content.sources;
                        topic.diagram = content.diagram;
                        if !same_brief {
                            // A new brief means a new picture; the old one
                            // no longer matches what the window says.
                            topic.image = content.image_brief.map(pending);
                        }
                        topic.updated_at_ms = now_ms;
                    }
                    for turn_id in content.evidence {
                        if !topic.source_turn_ids.contains(&turn_id) {
                            topic.source_turn_ids.push(turn_id);
                        }
                    }
                    topic.placement = placement;
                    placed.push(topic);
                }
                (Some(mut topic), Err(rejection)) => {
                    // Unchanged wording needs no fresh evidence; anything
                    // else keeps the notes it had.
                    let unchanged = topic.title == clip(&spec.title, MAX_TITLE_CHARS)
                        && topic.notes == clean_notes(&spec.notes)
                        && topic.diagram == diagram_of(&spec)
                        && topic.image.as_ref().map(|image| &image.brief) == image_brief_of(&spec).as_ref();
                    // (A topic with web notes always has evidence already,
                    // so it never reaches this branch unchanged-but-refused.)
                    if !unchanged {
                        rejections.push((spec.id, rejection));
                    }
                    topic.placement = placement;
                    placed.push(topic);
                }
                (None, Ok(content)) => placed.push(Topic {
                    id,
                    title: content.title,
                    notes: content.notes,
                    source_turn_ids: content.evidence,
                    sources: content.sources,
                    diagram: content.diagram,
                    image: content.image_brief.map(pending),
                    updated_at_ms: now_ms,
                    placement,
                }),
                (None, Err(rejection)) => rejections.push((spec.id, rejection)),
            }
        }

        // Whatever was not mentioned is put away, most recent first.
        let mut put_away = std::mem::take(&mut self.topics);
        for topic in &mut put_away {
            topic.placement = None;
        }
        put_away.sort_by_key(|topic| std::cmp::Reverse(topic.updated_at_ms));
        put_away.truncate(MAX_TOPICS.saturating_sub(placed.len()));
        placed.extend(put_away);
        self.topics = placed;

        (self.topics != before, rejections)
    }

    /// Records how an illustration turned out. Ignored, returning `false`,
    /// if the topic has since dropped or replaced that request.
    pub fn set_image_status(&mut self, topic_id: &str, brief: &str, status: ImageStatus) -> bool {
        match self
            .topics
            .iter_mut()
            .find(|topic| topic.id == topic_id)
            .and_then(|topic| topic.image.as_mut())
        {
            Some(image) if image.brief == brief && image.status != status => {
                image.status = status;
                true
            }
            _ => false,
        }
    }
}

fn pending(brief: String) -> TopicImage {
    TopicImage {
        brief,
        status: ImageStatus::Pending,
    }
}

fn diagram_of(spec: &WindowSpec) -> Option<String> {
    let diagram = spec.diagram.trim();
    // A truncated diagram would not parse, so an oversized one is dropped.
    (!diagram.is_empty() && diagram.chars().count() <= MAX_DIAGRAM_CHARS).then(|| diagram.to_owned())
}

fn image_brief_of(spec: &WindowSpec) -> Option<String> {
    if diagram_of(spec).is_some() {
        return None;
    }
    let brief = clip(&spec.image_brief, MAX_IMAGE_BRIEF_CHARS);
    (!brief.is_empty()).then_some(brief)
}

struct Content {
    title: String,
    notes: Vec<String>,
    evidence: Vec<String>,
    sources: Vec<Source>,
    diagram: Option<String>,
    image_brief: Option<String>,
}

fn content_of(
    spec: &WindowSpec,
    turn_is_citable: impl Fn(&str) -> bool,
    page_is_citable: impl Fn(&str) -> bool,
) -> Result<Content, Rejection> {
    let title = clip(&spec.title, MAX_TITLE_CHARS);
    if title.is_empty() {
        return Err(Rejection::NoTitle);
    }
    let mut notes = clean_notes(&spec.notes);
    let has_web_notes = notes.iter().any(|note| is_web_note(note));
    let sources: Vec<Source> = if has_web_notes {
        spec.sources
            .iter()
            .filter(|source| source.url.starts_with("https://") && page_is_citable(&source.url))
            .map(|source| Source {
                title: clip(&source.title, MAX_SOURCE_TITLE_CHARS),
                url: source.url.trim().to_owned(),
            })
            .take(MAX_SOURCES)
            .collect()
    } else {
        Vec::new()
    };
    if sources.is_empty() {
        // A web claim with no page behind it is not shown at all.
        notes.retain(|note| !is_web_note(note));
    }
    if notes.is_empty() {
        return Err(Rejection::NoNotes);
    }
    let evidence: Vec<String> = spec
        .turn_ids
        .iter()
        .filter(|turn_id| turn_is_citable(turn_id))
        .cloned()
        .collect();
    if evidence.is_empty() {
        return Err(Rejection::NoEvidence);
    }
    Ok(Content {
        title,
        notes,
        evidence,
        sources,
        diagram: diagram_of(spec),
        image_brief: image_brief_of(spec),
    })
}

/// A web finding appended to another note ("Open: …? Web: …") is separated
/// from it, so it is judged — and can be dropped — on its own.
fn split_web_claim(note: &str) -> Vec<&str> {
    let lower = note.to_ascii_lowercase();
    match lower.match_indices(WEB_NOTE_PREFIX).map(|(at, _)| at).find(|&at| at > 0) {
        Some(at) if note.is_char_boundary(at) => vec![&note[..at], &note[at..]],
        _ => vec![note],
    }
}

pub fn is_web_note(note: &str) -> bool {
    note.trim_start()
        .get(..WEB_NOTE_PREFIX.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(WEB_NOTE_PREFIX))
}

/// A URL without its query, fragment or trailing slash, for deciding whether
/// two links point at the same page (search results carry tracking
/// parameters).
fn comparable(url: &str) -> &str {
    let end = url.find(['?', '#']).unwrap_or(url.len());
    url[..end].trim().trim_end_matches('/')
}

/// `[label](https://…)` → `label`. Notes are plain text; sources are listed
/// separately.
fn strip_links(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let after_open = &rest[open + 1..];
        let link = after_open.find("](").and_then(|close| {
            let after_close = &after_open[close + 2..];
            after_close.find(')').map(|end| (close, close + 2 + end + 1))
        });
        match link {
            Some((label_len, consumed)) => {
                out.push_str(&rest[..open]);
                out.push_str(&after_open[..label_len]);
                rest = &after_open[consumed..];
            }
            None => {
                out.push_str(&rest[..=open]);
                rest = after_open;
            }
        }
    }
    out.push_str(rest);
    out
}

fn clean_notes(notes: &[String]) -> Vec<String> {
    notes
        .iter()
        .flat_map(|note| split_web_claim(note))
        .map(|note| clip(note, MAX_NOTE_CHARS))
        .filter(|note| !note.is_empty())
        .take(MAX_NOTES)
        .collect()
}

/// Lowercase letters, digits and single hyphens: safe as an identifier
/// anywhere, including a window label.
fn slug(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
        if out.len() >= MAX_ID_CHARS {
            break;
        }
    }
    out.trim_end_matches('-').to_owned()
}

fn clip(raw: &str, max_chars: usize) -> String {
    let text = strip_links(raw).split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= max_chars {
        return text;
    }
    let mut clipped: String = text.chars().take(max_chars - 1).collect();
    clipped.truncate(clipped.trim_end().len());
    clipped.push('…');
    clipped
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(id: &str, title: &str, notes: &[&str], turn_ids: &[&str]) -> WindowSpec {
        spec_at(id, title, notes, turn_ids, Zone::TopRight, Size::Medium)
    }

    fn spec_at(id: &str, title: &str, notes: &[&str], turn_ids: &[&str], zone: Zone, size: Size) -> WindowSpec {
        WindowSpec {
            id: id.into(),
            title: title.into(),
            notes: notes.iter().map(|s| s.to_string()).collect(),
            turn_ids: turn_ids.iter().map(|s| s.to_string()).collect(),
            sources: Vec::new(),
            diagram: String::new(),
            image_brief: String::new(),
            zone,
            size,
        }
    }

    /// Stands in for "the turns in the batch the agent was just shown".
    fn new(turn_id: &str) -> bool {
        turn_id.starts_with("customer-") || turn_id.starts_with("seller-")
    }

    fn no_web(_: &str) -> bool {
        false
    }

    fn web_spec(notes: &[&str], url: &str) -> WindowSpec {
        WindowSpec {
            sources: vec![Source { title: "Docs".into(), url: url.into() }],
            ..spec("ocp", "OpenShift support", notes, &["customer-0"])
        }
    }

    fn ids(board: &TopicBoard) -> Vec<&str> {
        board.topics().iter().map(|topic| topic.id.as_str()).collect()
    }

    #[test]
    fn the_agent_decides_which_windows_exist_where_and_in_what_order() {
        let mut board = TopicBoard::default();
        let (changed, rejections) = board.arrange(
            vec![
                spec_at("env", "Environment", &["AWS"], &["customer-0"], Zone::BottomLeft, Size::Small),
                spec_at("cmdb", "CMDB", &["Stale"], &["customer-0"], Zone::TopRight, Size::Large),
            ],
            new,
            no_web,
            10,
        );
        assert!(changed && rejections.is_empty());
        assert_eq!(ids(&board), ["env", "cmdb"]);
        assert_eq!(board.topics()[0].placement, Some(Placement { zone: Zone::BottomLeft, size: Size::Small }));
        assert_eq!(board.topics()[1].placement, Some(Placement { zone: Zone::TopRight, size: Size::Large }));
    }

    #[test]
    fn a_window_can_be_moved_and_resized_without_new_evidence() {
        let mut board = TopicBoard::default();
        board.arrange(vec![spec("cmdb", "CMDB", &["Stale"], &["customer-0"])], new, no_web, 10);
        let (changed, rejections) = board.arrange(
            vec![spec_at("cmdb", "CMDB", &["Stale"], &[], Zone::TopLeft, Size::Large)],
            |_| false,
            no_web,
            20,
        );
        assert!(changed && rejections.is_empty());
        let topic = &board.topics()[0];
        assert_eq!(topic.placement, Some(Placement { zone: Zone::TopLeft, size: Size::Large }));
        // Moving a window is not an update to what was said.
        assert_eq!(topic.updated_at_ms, 10);
    }

    #[test]
    fn leaving_a_topic_out_closes_its_window_but_keeps_its_notes() {
        let mut board = TopicBoard::default();
        board.arrange(
            vec![spec("a", "A", &["one"], &["customer-0"]), spec("b", "B", &["two"], &["customer-0"])],
            new,
            no_web,
            10,
        );
        board.arrange(vec![spec("b", "B", &["two"], &[])], |_| false, no_web, 20);
        assert_eq!(ids(&board), ["b", "a"]);
        assert_eq!(board.topics()[1].placement, None);
        assert_eq!(board.topics()[1].notes, ["one"]);
        // …and it can be brought back later exactly as it was.
        board.arrange(vec![spec("a", "A", &["one"], &[])], |_| false, no_web, 30);
        assert!(board.topics()[0].placement.is_some());
    }

    #[test]
    fn an_empty_arrangement_closes_every_window() {
        let mut board = TopicBoard::default();
        board.arrange(vec![spec("a", "A", &["one"], &["customer-0"])], new, no_web, 10);
        let (changed, _) = board.arrange(vec![], new, no_web, 20);
        assert!(changed);
        assert!(board.topics().iter().all(|topic| topic.placement.is_none()));
    }

    #[test]
    fn new_content_must_cite_a_turn_the_agent_could_have_read() {
        let mut board = TopicBoard::default();
        let (changed, rejections) = board.arrange(
            vec![
                spec("pricing", "Pricing", &["Budget is 2M"], &["made-up-7"]),
                spec("budget", "Budget", &["2M"], &[]),
            ],
            new,
            no_web,
            0,
        );
        assert!(!changed);
        assert_eq!(rejections.len(), 2);
        assert!(rejections.iter().all(|(_, rejection)| *rejection == Rejection::NoEvidence));
        assert!(board.topics().is_empty());
    }

    #[test]
    fn rewording_without_evidence_keeps_the_old_notes_but_still_moves_the_window() {
        let mut board = TopicBoard::default();
        board.arrange(vec![spec("cmdb", "CMDB", &["Stale"], &["customer-0"])], new, no_web, 10);
        let (_, rejections) = board.arrange(
            vec![spec_at("cmdb", "CMDB", &["Budget approved"], &["ghost"], Zone::BottomRight, Size::Small)],
            |_| false,
            no_web,
            20,
        );
        assert_eq!(rejections, [("cmdb".to_string(), Rejection::NoEvidence)]);
        let topic = &board.topics()[0];
        assert_eq!(topic.notes, ["Stale"]);
        assert_eq!(topic.placement.unwrap().zone, Zone::BottomRight);
    }

    #[test]
    fn topics_can_be_merged_by_citing_turns_already_on_the_board() {
        let mut board = TopicBoard::default();
        board.arrange(
            vec![spec("aws", "AWS", &["Runs AWS"], &["customer-0"]), spec("ocp", "OpenShift", &["Runs OpenShift"], &["customer-1"])],
            new,
            no_web,
            10,
        );
        // A later batch: neither turn is new any more.
        let (changed, rejections) = board.arrange(
            vec![spec("environment", "Environment", &["Runs AWS", "Runs OpenShift"], &["customer-0", "customer-1"])],
            |_| false,
            no_web,
            20,
        );
        assert!(changed && rejections.is_empty());
        assert_eq!(board.topics()[0].id, "environment");
        assert_eq!(board.topics()[0].source_turn_ids, ["customer-0", "customer-1"]);
        assert!(board.topics()[1..].iter().all(|topic| topic.placement.is_none()));
    }

    #[test]
    fn an_update_replaces_notes_and_accumulates_evidence() {
        let mut board = TopicBoard::default();
        board.arrange(vec![spec("env", "Environment", &["AWS"], &["customer-0"])], new, no_web, 1);
        board.arrange(vec![spec("env", "Current environment", &["AWS", "OpenShift"], &["customer-3"])], new, no_web, 2);
        let topic = &board.topics()[0];
        assert_eq!(topic.title, "Current environment");
        assert_eq!(topic.notes, ["AWS", "OpenShift"]);
        assert_eq!(topic.source_turn_ids, ["customer-0", "customer-3"]);
        assert_eq!(topic.updated_at_ms, 2);
    }

    #[test]
    fn restating_the_same_arrangement_reports_no_change() {
        let mut board = TopicBoard::default();
        board.arrange(vec![spec("a", "A", &["one"], &["customer-0"])], new, no_web, 10);
        let (changed, rejections) = board.arrange(vec![spec("A", "A", &["one"], &[])], |_| false, no_web, 99);
        assert!(!changed && rejections.is_empty());
    }

    #[test]
    fn duplicates_and_windows_beyond_the_limit_are_refused() {
        let mut board = TopicBoard::default();
        let mut windows: Vec<WindowSpec> = (0..MAX_WINDOWS + 2)
            .map(|index| spec(&format!("t{index}"), "T", &["n"], &["customer-0"]))
            .collect();
        windows.insert(1, spec("T0", "Again", &["n"], &["customer-0"]));
        let (_, rejections) = board.arrange(windows, new, no_web, 0);
        assert_eq!(board.topics().iter().filter(|topic| topic.placement.is_some()).count(), MAX_WINDOWS);
        assert_eq!(rejections.iter().filter(|(_, r)| *r == Rejection::Duplicate).count(), 1);
        assert_eq!(rejections.iter().filter(|(_, r)| *r == Rejection::TooMany).count(), 2);
    }

    #[test]
    fn empty_fields_are_rejected() {
        let mut board = TopicBoard::default();
        let (_, rejections) = board.arrange(
            vec![
                spec("!!!", "T", &["n"], &["customer-0"]),
                spec("a", "  ", &["n"], &["customer-0"]),
                spec("b", "T", &[" ", ""], &["customer-0"]),
            ],
            new,
            no_web,
            0,
        );
        let reasons: Vec<Rejection> = rejections.into_iter().map(|(_, reason)| reason).collect();
        assert_eq!(reasons, [Rejection::NoId, Rejection::NoTitle, Rejection::NoNotes]);
        assert!(board.topics().is_empty());
    }

    #[test]
    fn oversized_content_is_clipped_to_fit_a_small_window() {
        let mut board = TopicBoard::default();
        let long = "word ".repeat(100);
        let notes: Vec<&str> = std::iter::repeat_n(long.as_str(), 9).collect();
        board.arrange(vec![spec("a", &long, &notes, &["customer-0"])], new, no_web, 0);
        let topic = &board.topics()[0];
        assert_eq!(topic.notes.len(), MAX_NOTES);
        assert!(topic.title.chars().count() <= MAX_TITLE_CHARS);
        assert!(topic.notes[0].chars().count() <= MAX_NOTE_CHARS);
        assert!(topic.notes[0].ends_with('…'));
    }

    #[test]
    fn the_board_forgets_only_the_stalest_put_away_topics() {
        let mut board = TopicBoard::default();
        for index in 0..MAX_TOPICS + 3 {
            // Each round shows one new topic and puts the previous one away.
            board.arrange(
                vec![spec(&format!("t{index}"), "T", &["n"], &["customer-0"])],
                new,
                no_web,
                index as u64,
            );
        }
        assert_eq!(board.topics().len(), MAX_TOPICS);
        let kept = ids(&board);
        assert!(kept.contains(&format!("t{}", MAX_TOPICS + 2).as_str()));
        assert!(!kept.contains(&"t0"));
    }

    #[test]
    fn a_web_note_is_kept_only_with_a_page_a_search_really_returned() {
        let notes = ["Open: Does Discovery support OpenShift?", "Web: Discovery documents an OpenShift provider."];
        let retrieved = |url: &str| comparable(url) == "https://docs.example.com/discovery/openshift";

        let mut board = TopicBoard::default();
        board.arrange(vec![web_spec(&notes, "https://docs.example.com/discovery/openshift/?utm_source=x")], new, retrieved, 0);
        assert_eq!(board.topics()[0].notes.len(), 2);
        assert_eq!(board.topics()[0].sources.len(), 1);

        // Same claim, but the page was never returned by a search.
        let mut board = TopicBoard::default();
        board.arrange(vec![web_spec(&notes, "https://made-up.example.com/")], new, retrieved, 0);
        assert_eq!(board.topics()[0].notes, ["Open: Does Discovery support OpenShift?"]);
        assert!(board.topics()[0].sources.is_empty());
    }

    #[test]
    fn a_window_holding_only_an_unsupported_web_claim_is_refused() {
        let mut board = TopicBoard::default();
        let (_, rejections) = board.arrange(
            vec![web_spec(&["Web: It is supported."], "http://insecure.example.com/")],
            new,
            |_| true,
            0,
        );
        assert_eq!(rejections, [("ocp".to_string(), Rejection::NoNotes)]);
    }

    #[test]
    fn sources_without_web_notes_are_not_kept() {
        let mut board = TopicBoard::default();
        board.arrange(vec![web_spec(&["Customer: runs OpenShift"], "https://docs.example.com/a")], new, |_| true, 0);
        assert!(board.topics()[0].sources.is_empty());
    }

    #[test]
    fn a_web_claim_tacked_onto_another_note_is_judged_separately() {
        let mut board = TopicBoard::default();
        board.arrange(
            vec![web_spec(&["Open: Does it support OpenShift? Web: Yes, fully."], "https://never-searched.example.com/")],
            new,
            no_web,
            0,
        );
        // The question was asked; the unsourced answer is not shown.
        assert_eq!(board.topics()[0].notes, ["Open: Does it support OpenShift?"]);
    }

    #[test]
    fn link_markup_is_removed_from_notes() {
        assert_eq!(
            clip("Web: Supported ([docs.bmc.com](https://docs.bmc.com/x?utm=1)).", 200),
            "Web: Supported (docs.bmc.com)."
        );
        assert_eq!(clip("Uses [brackets] normally", 200), "Uses [brackets] normally");
    }

    fn visual_spec(diagram: &str, image_brief: &str, turn_ids: &[&str]) -> WindowSpec {
        WindowSpec {
            diagram: diagram.into(),
            image_brief: image_brief.into(),
            ..spec_at("env", "Environment", &["Runs AWS"], turn_ids, Zone::TopRight, Size::Tall)
        }
    }

    #[test]
    fn a_diagram_is_content_and_needs_evidence_like_any_note() {
        let mut board = TopicBoard::default();
        board.arrange(vec![visual_spec("flowchart TD\n  A[AWS] --> C[CMDB]", "", &["customer-0"])], new, no_web, 1);
        assert_eq!(board.topics()[0].diagram.as_deref(), Some("flowchart TD\n  A[AWS] --> C[CMDB]"));

        // Changing the diagram later without citing anything is refused.
        let (_, rejections) = board.arrange(vec![visual_spec("flowchart TD\n  X --> Y", "", &["ghost"])], |_| false, no_web, 2);
        assert_eq!(rejections.len(), 1);
        assert!(board.topics()[0].diagram.as_deref().unwrap().contains("AWS"));
    }

    #[test]
    fn an_oversized_diagram_is_dropped_rather_than_cut_in_half() {
        let mut board = TopicBoard::default();
        let huge = format!("flowchart TD\n{}", "  A --> B\n".repeat(400));
        board.arrange(vec![visual_spec(&huge, "", &["customer-0"])], new, no_web, 1);
        assert_eq!(board.topics()[0].diagram, None);
        assert_eq!(board.topics()[0].notes, ["Runs AWS"]);
    }

    #[test]
    fn an_image_request_starts_pending_and_is_tracked_to_its_outcome() {
        let mut board = TopicBoard::default();
        board.arrange(vec![visual_spec("", "AWS and OpenShift feeding a CMDB", &["customer-0"])], new, no_web, 1);
        let image = board.topics()[0].image.clone().unwrap();
        assert_eq!(image.status, ImageStatus::Pending);

        assert!(board.set_image_status("env", &image.brief, ImageStatus::Ready));
        // Restating the same window keeps the finished picture.
        board.arrange(vec![visual_spec("", "AWS and OpenShift feeding a CMDB", &[])], |_| false, no_web, 2);
        assert_eq!(board.topics()[0].image.as_ref().unwrap().status, ImageStatus::Ready);
    }

    #[test]
    fn a_result_for_a_request_that_was_replaced_is_ignored() {
        let mut board = TopicBoard::default();
        board.arrange(vec![visual_spec("", "first idea", &["customer-0"])], new, no_web, 1);
        board.arrange(vec![visual_spec("", "second idea", &["customer-1"])], new, no_web, 2);
        assert!(!board.set_image_status("env", "first idea", ImageStatus::Ready));
        assert_eq!(board.topics()[0].image.as_ref().unwrap().status, ImageStatus::Pending);
        assert!(!board.set_image_status("gone", "second idea", ImageStatus::Ready));
    }

    #[test]
    fn a_window_shows_a_diagram_or_an_image_never_both() {
        let mut board = TopicBoard::default();
        board.arrange(vec![visual_spec("flowchart TD\n  A --> B", "also draw this", &["customer-0"])], new, no_web, 1);
        assert!(board.topics()[0].diagram.is_some());
        assert_eq!(board.topics()[0].image, None);
    }
}
