//! Meeting intelligence that runs on finalized turns.

mod illustrator;
mod responses;
mod topics;

pub use illustrator::Illustrator;
pub use responses::{IntelError, ResponsesClient, Structured, StructuredRequest};
pub use topics::{track_topics, Abilities, Update};
