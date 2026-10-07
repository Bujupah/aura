//! The summary produced when a meeting ends.
//!
//! Every listed item must cite a turn of the meeting or it is left out. The
//! overview and the suggested next step are the model's own wording and are
//! presented as such.

use serde::{Deserialize, Serialize};

use crate::topics::clip;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryItem {
    pub text: String,
    pub source_turn_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NextStep {
    pub text: String,
    pub why: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingSummary {
    pub headline: String,
    pub overview: String,
    pub environment: Vec<SummaryItem>,
    pub pain_points: Vec<SummaryItem>,
    pub requirements: Vec<SummaryItem>,
    pub open_questions: Vec<SummaryItem>,
    pub commitments: Vec<SummaryItem>,
    pub next_step: Option<NextStep>,
}

/// One item as the model returns it. Untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposedItem {
    pub text: String,
    pub turn_ids: Vec<String>,
}

/// What the summarizing model returns. Untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryProposal {
    pub headline: String,
    pub overview: String,
    pub environment: Vec<ProposedItem>,
    pub pain_points: Vec<ProposedItem>,
    pub requirements: Vec<ProposedItem>,
    pub open_questions: Vec<ProposedItem>,
    pub commitments: Vec<ProposedItem>,
    pub next_step: String,
    pub next_step_why: String,
}

const MAX_ITEMS: usize = 10;
const MAX_ITEM_CHARS: usize = 220;

impl MeetingSummary {
    /// Keeps what the transcript supports. `turn_exists` answers whether a
    /// turn id belongs to this meeting.
    pub fn from_proposal(proposal: SummaryProposal, turn_exists: impl Fn(&str) -> bool) -> Self {
        let supported = |items: Vec<ProposedItem>| -> Vec<SummaryItem> {
            items
                .into_iter()
                .filter_map(|item| {
                    let text = clip(&item.text, MAX_ITEM_CHARS);
                    let source_turn_ids: Vec<String> =
                        item.turn_ids.into_iter().filter(|turn_id| turn_exists(turn_id)).collect();
                    (!text.is_empty() && !source_turn_ids.is_empty()).then_some(SummaryItem { text, source_turn_ids })
                })
                .take(MAX_ITEMS)
                .collect()
        };
        let next_step = clip(&proposal.next_step, 240);
        Self {
            headline: clip(&proposal.headline, 100),
            overview: clip(&proposal.overview, 700),
            environment: supported(proposal.environment),
            pain_points: supported(proposal.pain_points),
            requirements: supported(proposal.requirements),
            open_questions: supported(proposal.open_questions),
            commitments: supported(proposal.commitments),
            next_step: (!next_step.is_empty()).then(|| NextStep {
                text: next_step,
                why: clip(&proposal.next_step_why, 240),
            }),
        }
    }

    /// The summary as Markdown, for pasting into notes, an email or a CRM.
    pub fn to_markdown(&self) -> String {
        let mut out = format!("# {}\n\n{}\n", self.headline, self.overview);
        let sections = [
            ("Customer environment", &self.environment),
            ("Pain points", &self.pain_points),
            ("Requirements", &self.requirements),
            ("Open questions", &self.open_questions),
            ("Commitments", &self.commitments),
        ];
        for (title, items) in sections {
            if items.is_empty() {
                continue;
            }
            out.push_str(&format!("\n## {title}\n\n"));
            for item in items {
                out.push_str(&format!("- {}\n", item.text));
            }
        }
        if let Some(next_step) = &self.next_step {
            out.push_str(&format!("\n## Suggested next step\n\n{}\n", next_step.text));
            if !next_step.why.is_empty() {
                out.push_str(&format!("\n_{}_\n", next_step.why));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(text: &str, turn_ids: &[&str]) -> ProposedItem {
        ProposedItem {
            text: text.into(),
            turn_ids: turn_ids.iter().map(|id| id.to_string()).collect(),
        }
    }

    fn proposal() -> SummaryProposal {
        SummaryProposal {
            headline: "Discovery call on CMDB accuracy".into(),
            overview: "The customer's CMDB goes stale and alerts are noisy.".into(),
            environment: vec![item("Runs AWS and OpenShift", &["customer-0"])],
            pain_points: vec![item("CMDB is out of date", &["customer-0"]), item("Budget was cut", &["ghost-4"])],
            requirements: vec![],
            open_questions: vec![item("Does Discovery support OpenShift?", &["customer-2", "ghost-1"])],
            commitments: vec![item("Send a reference architecture by Friday", &["seller-3"]), item(" ", &["seller-3"])],
            next_step: "Schedule a discovery workshop.".into(),
            next_step_why: "Scale and ownership are still unknown.".into(),
        }
    }

    fn known(turn_id: &str) -> bool {
        turn_id.starts_with("customer-") || turn_id.starts_with("seller-")
    }

    #[test]
    fn items_the_transcript_does_not_support_are_left_out() {
        let summary = MeetingSummary::from_proposal(proposal(), known);
        assert_eq!(summary.pain_points.len(), 1, "the invented budget cut is gone");
        assert_eq!(summary.pain_points[0].text, "CMDB is out of date");
        assert_eq!(summary.open_questions[0].source_turn_ids, ["customer-2"]);
        assert_eq!(summary.commitments.len(), 1, "an empty item is not a commitment");
        assert!(summary.next_step.is_some());
    }

    #[test]
    fn markdown_has_only_the_sections_with_content() {
        let markdown = MeetingSummary::from_proposal(proposal(), known).to_markdown();
        assert!(markdown.starts_with("# Discovery call on CMDB accuracy\n"));
        assert!(markdown.contains("## Commitments\n\n- Send a reference architecture by Friday\n"));
        assert!(markdown.contains("## Suggested next step\n\nSchedule a discovery workshop.\n"));
        assert!(!markdown.contains("## Requirements"));
        assert!(!markdown.contains("Budget"));
    }

    #[test]
    fn an_empty_next_step_is_absent_rather_than_blank() {
        let summary = MeetingSummary::from_proposal(SummaryProposal { next_step: "  ".into(), ..proposal() }, known);
        assert_eq!(summary.next_step, None);
        assert!(!summary.to_markdown().contains("Suggested next step"));
    }
}
