//! The one next move shown to the seller, and the on-request list of what
//! is still unknown.
//!
//! An advisor model proposes; this module decides what is shown. A move must
//! point at turns that exist, so the seller can always see what prompted it,
//! and "nothing right now" is a normal answer: silence is better than noise.

use serde::{Deserialize, Serialize};

use crate::topics::clip;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AdviceKind {
    /// A question worth asking the customer next.
    Ask,
    /// Something worth saying.
    Say,
    /// Something the seller said that should be confirmed before it stands.
    Caution,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Advice {
    pub kind: AdviceKind,
    pub text: String,
    pub why: String,
    pub source_turn_ids: Vec<String>,
    pub at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProposedKind {
    Ask,
    Say,
    Caution,
    /// Nothing is worth interrupting the seller for.
    None,
}

/// What the advisor model returns. Untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdviceProposal {
    pub kind: ProposedKind,
    pub text: String,
    pub why: String,
    pub turn_ids: Vec<String>,
}

const MAX_TEXT_CHARS: usize = 150;
const MAX_WHY_CHARS: usize = 170;

/// Turns a proposal into advice, or into nothing: when the advisor has
/// nothing to say, says it emptily, or cites no turn of this meeting.
pub fn accept(proposal: AdviceProposal, turn_exists: impl Fn(&str) -> bool, now_ms: u64) -> Option<Advice> {
    let kind = match proposal.kind {
        ProposedKind::Ask => AdviceKind::Ask,
        ProposedKind::Say => AdviceKind::Say,
        ProposedKind::Caution => AdviceKind::Caution,
        ProposedKind::None => return None,
    };
    let text = clip(&proposal.text, MAX_TEXT_CHARS);
    let source_turn_ids: Vec<String> = proposal
        .turn_ids
        .into_iter()
        .filter(|turn_id| turn_exists(turn_id))
        .collect();
    if text.is_empty() || source_turn_ids.is_empty() {
        return None;
    }
    Some(Advice {
        kind,
        text,
        why: clip(&proposal.why, MAX_WHY_CHARS),
        source_turn_ids,
        at_ms: now_ms,
    })
}

/// An answer to "what are we missing?". Unlike notes, this is the advisor's
/// judgement about what has *not* been said, so it cannot cite evidence and
/// is always presented as a suggestion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Gaps {
    /// One line on what is already understood.
    pub understood: String,
    pub missing: Vec<String>,
    /// The single gap to close first.
    pub priority: String,
}

pub const MAX_GAPS: usize = 5;

impl Gaps {
    /// Trims the model's answer to what fits the overlay.
    pub fn tidy(self) -> Self {
        Self {
            understood: clip(&self.understood, 200),
            missing: self
                .missing
                .iter()
                .map(|gap| clip(gap, 120))
                .filter(|gap| !gap.is_empty())
                .take(MAX_GAPS)
                .collect(),
            priority: clip(&self.priority, 160),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proposal(kind: ProposedKind, text: &str, turn_ids: &[&str]) -> AdviceProposal {
        AdviceProposal {
            kind,
            text: text.into(),
            why: "It decides whether discovery belongs in the picture.".into(),
            turn_ids: turn_ids.iter().map(|id| id.to_string()).collect(),
        }
    }

    fn known(turn_id: &str) -> bool {
        turn_id.starts_with("customer-")
    }

    #[test]
    fn a_grounded_question_becomes_advice() {
        let advice = accept(
            proposal(ProposedKind::Ask, "How is CI data reconciled today?", &["customer-3"]),
            known,
            42,
        )
        .unwrap();
        assert_eq!(advice.kind, AdviceKind::Ask);
        assert_eq!(advice.text, "How is CI data reconciled today?");
        assert_eq!(advice.source_turn_ids, ["customer-3"]);
        assert_eq!(advice.at_ms, 42);
    }

    #[test]
    fn having_nothing_to_say_is_a_valid_answer() {
        assert_eq!(accept(proposal(ProposedKind::None, "ignored", &["customer-1"]), known, 0), None);
    }

    #[test]
    fn advice_that_cites_no_real_turn_or_says_nothing_is_dropped() {
        assert_eq!(accept(proposal(ProposedKind::Ask, "Ask about budget", &["invented-9"]), known, 0), None);
        assert_eq!(accept(proposal(ProposedKind::Ask, "Ask about budget", &[]), known, 0), None);
        assert_eq!(accept(proposal(ProposedKind::Say, "   ", &["customer-1"]), known, 0), None);
    }

    #[test]
    fn long_advice_is_clipped_to_stay_glanceable() {
        let long = "word ".repeat(80);
        let advice = accept(proposal(ProposedKind::Ask, &long, &["customer-1"]), known, 0).unwrap();
        assert!(advice.text.chars().count() <= MAX_TEXT_CHARS);
    }

    #[test]
    fn gaps_are_limited_to_a_handful() {
        let gaps = Gaps {
            understood: "The monitoring problem.".into(),
            missing: (0..9).map(|n| format!("Gap {n}")).chain(["  ".to_owned()]).collect(),
            priority: "Who owns the CMDB?".into(),
        }
        .tidy();
        assert_eq!(gaps.missing.len(), MAX_GAPS);
    }
}
