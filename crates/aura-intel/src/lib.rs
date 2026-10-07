//! Meeting intelligence that runs on finalized turns.

mod advisor;
mod illustrator;
mod responses;
mod summary;
mod topics;

pub use advisor::{track_advice, AdvisorRequest, AdvisorSetup, AdvisorUpdate, Ask};
pub use illustrator::Illustrator;
pub use responses::{IntelError, ResponsesClient, Structured, StructuredRequest, WebSearch};
pub use summary::summarize;
pub use topics::{track_topics, Abilities, Control, Earlier, Update};

/// A prompt file without its front matter.
pub(crate) fn prompt_body(source: &'static str) -> &'static str {
    source
        .strip_prefix("---")
        .and_then(|rest| rest.split_once("\n---\n"))
        .map_or(source, |(_, body)| body)
        .trim()
}
