//! The one next move shown to the seller, and the on-request list of what
//! is still unknown.
//!
//! An advisor model proposes; this module decides what is shown. A move must
//! point at turns that exist, so the seller can always see what prompted it,
//! and "nothing right now" is a normal answer: silence is better than noise.

use serde::{Deserialize, Serialize};

use crate::topics::{clip, Source};

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

/// Whether an on-request answer is backed by a source Aura trusts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Verification {
    /// A page from a trusted source, returned by a search in this request,
    /// supports the answer.
    Verified,
    /// A factual answer with no trusted source behind it.
    Unverified,
    /// The answer restates the meeting; there is nothing to verify.
    NotApplicable,
}

/// The reply to something the seller asked Aura for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Answer {
    pub title: String,
    pub summary: String,
    pub points: Vec<String>,
    /// Words the seller could say aloud, when that helps.
    pub say_this: String,
    /// Mermaid source, when a picture answers better than a list.
    pub diagram: Option<String>,
    pub verification: Verification,
    pub sources: Vec<Source>,
}

/// What the advisor model returns for a request. Untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerProposal {
    pub title: String,
    pub summary: String,
    pub points: Vec<String>,
    pub say_this: String,
    pub diagram: String,
    pub verification: Verification,
    pub sources: Vec<Source>,
}

pub const MAX_POINTS: usize = 7;
const MAX_DIAGRAM_CHARS: usize = 1_500;

/// Decides what of a proposed answer stands.
///
/// Sources are kept only if a search in this request really returned them.
/// "Verified" is the model's claim; it holds only if one of the kept sources
/// is on a trusted host. With no trusted hosts to check against, nothing can
/// be verified.
pub fn accept_answer(
    proposal: AnswerProposal,
    url_was_retrieved: impl Fn(&str) -> bool,
    trusted_hosts: &[&str],
) -> Answer {
    let sources: Vec<Source> = proposal
        .sources
        .into_iter()
        .filter(|source| source.url.starts_with("https://") && url_was_retrieved(&source.url))
        .map(|source| Source {
            title: clip(&source.title, 80),
            url: source.url.trim().to_owned(),
        })
        .take(3)
        .collect();
    let trusted = sources
        .iter()
        .any(|source| host_of(&source.url).is_some_and(|host| is_on(host, trusted_hosts)));
    let verification = match proposal.verification {
        Verification::Verified if trusted => Verification::Verified,
        Verification::Verified => Verification::Unverified,
        other => other,
    };
    let diagram = proposal.diagram.trim();
    Answer {
        title: clip(&proposal.title, 60),
        summary: clip(&proposal.summary, 320),
        points: proposal
            .points
            .iter()
            .map(|point| clip(point, 170))
            .filter(|point| !point.is_empty())
            .take(MAX_POINTS)
            .collect(),
        say_this: clip(&proposal.say_this, 240),
        diagram: (!diagram.is_empty() && diagram.chars().count() <= MAX_DIAGRAM_CHARS).then(|| diagram.to_owned()),
        verification,
        sources,
    }
}

fn host_of(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://")?;
    let host = rest.split(['/', '?', '#']).next()?;
    // Drop any port, and refuse anything carrying credentials.
    (!host.contains('@')).then(|| host.split(':').next().unwrap_or(host))
}

/// Whether `host` is one of `domains` or a subdomain of one.
fn is_on(host: &str, domains: &[&str]) -> bool {
    let host = host.to_ascii_lowercase();
    domains
        .iter()
        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
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

    fn answer(verification: Verification, urls: &[&str]) -> AnswerProposal {
        AnswerProposal {
            title: "OpenShift support".into(),
            summary: "The documentation describes an OpenShift discovery provider.".into(),
            points: vec!["Provider: Kubernetes/OpenShift Cluster".into()],
            say_this: "Let me confirm the exact versions.".into(),
            diagram: String::new(),
            verification,
            sources: urls.iter().map(|url| Source { title: "Docs".into(), url: url.to_string() }).collect(),
        }
    }

    const TRUSTED: &[&str] = &["bmc.com"];

    #[test]
    fn verified_needs_a_trusted_page_that_a_search_really_returned() {
        let retrieved = |url: &str| url.starts_with("https://docs.bmc.com/") || url.starts_with("https://blog.example.com/");

        let good = accept_answer(answer(Verification::Verified, &["https://docs.bmc.com/discovery/openshift"]), retrieved, TRUSTED);
        assert_eq!(good.verification, Verification::Verified);
        assert_eq!(good.sources.len(), 1);

        // A real page, but not a trusted one.
        let blog = accept_answer(answer(Verification::Verified, &["https://blog.example.com/post"]), retrieved, TRUSTED);
        assert_eq!(blog.verification, Verification::Unverified);
        assert_eq!(blog.sources.len(), 1, "the source is still shown, as a lead");

        // A trusted-looking page that no search returned.
        let invented = accept_answer(answer(Verification::Verified, &["https://docs.bmc.com/made-up"]), |_| false, TRUSTED);
        assert_eq!(invented.verification, Verification::Unverified);
        assert!(invented.sources.is_empty());
    }

    #[test]
    fn a_lookalike_host_is_not_trusted() {
        let retrieved = |_: &str| true;
        for url in ["https://bmc.com.evil.example/x", "https://notbmc.com/x", "https://user@docs.bmc.com/x"] {
            let result = accept_answer(answer(Verification::Verified, &[url]), retrieved, TRUSTED);
            assert_eq!(result.verification, Verification::Unverified, "{url} was trusted");
        }
        let exact = accept_answer(answer(Verification::Verified, &["https://bmc.com/x"]), retrieved, TRUSTED);
        assert_eq!(exact.verification, Verification::Verified);
    }

    #[test]
    fn without_trusted_hosts_nothing_can_be_verified() {
        let result = accept_answer(answer(Verification::Verified, &["https://docs.bmc.com/x"]), |_| true, &[]);
        assert_eq!(result.verification, Verification::Unverified);
        let recap = accept_answer(answer(Verification::NotApplicable, &[]), |_| false, &[]);
        assert_eq!(recap.verification, Verification::NotApplicable);
    }

    #[test]
    fn an_answer_is_trimmed_to_fit_the_overlay() {
        let mut proposal = answer(Verification::NotApplicable, &[]);
        proposal.points = (0..12).map(|n| format!("Point {n}")).collect();
        proposal.diagram = format!("flowchart TD\n{}", "A --> B\n".repeat(400));
        let result = accept_answer(proposal, |_| false, &[]);
        assert_eq!(result.points.len(), MAX_POINTS);
        assert_eq!(result.diagram, None, "an oversized diagram is dropped, not cut in half");
    }
}
