//! Turns a stream of transcript fragments into speaker turns.
//!
//! The realtime API reports text in small timestamped fragments and marks no
//! turn boundaries, so Aura decides where one utterance ends: after a pause,
//! or at a sentence end once a turn has run long. All times are milliseconds
//! on the meeting clock.

use serde::{Deserialize, Serialize};

/// Who a stream belongs to. This comes from which device the audio was
/// captured on, never from a model's guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Speaker {
    /// The microphone.
    Seller,
    /// System audio: the customer or any other meeting participant.
    Customer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    /// Stable for the life of the turn, so a UI can update it in place.
    pub id: String,
    pub speaker: Speaker,
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
    /// False while the speaker may still be adding to it.
    pub is_final: bool,
    /// What an interpreter said on the speaker's behalf, when their speech
    /// is being translated for the other side.
    pub translation: Option<String>,
}

/// A pause this long ends a turn.
const TURN_GAP_MS: u64 = 1_200;
/// Past this length a turn is closed at the next sentence end, so a long
/// monologue still reaches the intelligence layer in usable pieces.
const LONG_TURN_MS: u64 = 15_000;

pub struct TurnAssembler {
    speaker: Speaker,
    current: Option<Turn>,
    /// The most recently closed turn. An interpretation trails the speech it
    /// renders, so its last words often arrive after the turn has closed.
    last_closed: Option<Turn>,
    next_index: u64,
}

impl TurnAssembler {
    pub fn new(speaker: Speaker) -> Self {
        Self {
            speaker,
            current: None,
            last_closed: None,
            next_index: 0,
        }
    }

    pub fn speaker(&self) -> Speaker {
        self.speaker
    }

    /// Adds a fragment. Returns the turns that changed, oldest first: at most
    /// one finalized turn followed by the turn still in progress.
    pub fn push(&mut self, delta: &str, start_ms: u64, end_ms: u64) -> Vec<Turn> {
        let mut changed = Vec::with_capacity(2);
        if delta.is_empty() {
            return changed;
        }
        if let Some(current) = &self.current {
            let paused = start_ms.saturating_sub(current.end_ms) >= TURN_GAP_MS;
            let ran_long = current.end_ms.saturating_sub(current.start_ms) >= LONG_TURN_MS
                && ends_sentence(&current.text);
            if paused || ran_long {
                changed.extend(self.finish());
            }
        }

        let turn = self.current.get_or_insert_with(|| {
            let turn = Turn {
                id: format!("{}-{}", prefix(self.speaker), self.next_index),
                speaker: self.speaker,
                text: String::new(),
                start_ms,
                end_ms,
                is_final: false,
                translation: None,
            };
            self.next_index += 1;
            turn
        });
        if turn.text.is_empty() {
            turn.text.push_str(delta.trim_start());
        } else {
            turn.text.push_str(delta);
        }
        turn.end_ms = turn.end_ms.max(end_ms);
        if !turn.text.trim().is_empty() {
            changed.push(turn.clone());
        }
        changed
    }

    /// Closes the turn in progress if the speaker has been quiet since
    /// `now_ms`. Call periodically; fragments alone cannot reveal silence.
    pub fn close_if_quiet(&mut self, now_ms: u64) -> Option<Turn> {
        let quiet = self
            .current
            .as_ref()
            .is_some_and(|turn| now_ms.saturating_sub(turn.end_ms) >= TURN_GAP_MS);
        if quiet {
            self.finish()
        } else {
            None
        }
    }

    /// Closes the turn in progress, if any. Use when the stream ends.
    pub fn finish(&mut self) -> Option<Turn> {
        let mut turn = self.current.take()?;
        turn.text = turn.text.trim().to_owned();
        if turn.text.is_empty() {
            return None;
        }
        turn.is_final = true;
        self.last_closed = Some(turn.clone());
        Some(turn)
    }

    /// Adds interpreted text to the turn it belongs to: the one in progress,
    /// or else the one that just closed. Returns the updated turn.
    pub fn push_translation(&mut self, text: &str) -> Option<Turn> {
        let turn = match self.current.as_mut() {
            Some(turn) => turn,
            None => self.last_closed.as_mut()?,
        };
        let translation = turn.translation.get_or_insert_with(String::new);
        if translation.is_empty() {
            translation.push_str(text.trim_start());
        } else {
            translation.push_str(text);
        }
        if translation.trim().is_empty() {
            turn.translation = None;
            return None;
        }
        Some(turn.clone())
    }
}

fn prefix(speaker: Speaker) -> &'static str {
    match speaker {
        Speaker::Seller => "seller",
        Speaker::Customer => "customer",
    }
}

fn ends_sentence(text: &str) -> bool {
    text.trim_end().ends_with(['.', '?', '!'])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragments_accumulate_into_one_growing_turn() {
        let mut assembler = TurnAssembler::new(Speaker::Customer);
        let first = assembler.push(" Our", 800, 1000);
        let second = assembler.push(" CMDB", 1200, 1400);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].text, "Our");
        assert_eq!(second[0].id, first[0].id);
        assert_eq!(second[0].text, "Our CMDB");
        assert_eq!((second[0].start_ms, second[0].end_ms), (800, 1400));
        assert!(!second[0].is_final);
    }

    #[test]
    fn word_pieces_join_without_inserted_spaces() {
        let mut assembler = TurnAssembler::new(Speaker::Customer);
        assembler.push(" Open", 0, 200);
        let turns = assembler.push("Shift", 200, 400);
        assert_eq!(turns[0].text, "OpenShift");
    }

    #[test]
    fn a_pause_finalizes_the_turn_and_starts_a_new_one() {
        let mut assembler = TurnAssembler::new(Speaker::Seller);
        assembler.push(" Hello.", 0, 500);
        let changed = assembler.push(" Next point", 500 + TURN_GAP_MS, 2500);
        assert_eq!(changed.len(), 2);
        assert!(changed[0].is_final);
        assert_eq!(changed[0].text, "Hello.");
        assert_eq!(changed[0].id, "seller-0");
        assert!(!changed[1].is_final);
        assert_eq!(changed[1].id, "seller-1");
    }

    #[test]
    fn silence_closes_a_turn_even_without_a_following_fragment() {
        let mut assembler = TurnAssembler::new(Speaker::Customer);
        assembler.push(" Does it support OpenShift?", 0, 2000);
        assert_eq!(assembler.close_if_quiet(2000 + TURN_GAP_MS - 1), None);
        let closed = assembler.close_if_quiet(2000 + TURN_GAP_MS).unwrap();
        assert!(closed.is_final);
        assert_eq!(closed.text, "Does it support OpenShift?");
        assert_eq!(assembler.close_if_quiet(60_000), None);
    }

    #[test]
    fn a_long_monologue_is_split_at_a_sentence_end() {
        let mut assembler = TurnAssembler::new(Speaker::Customer);
        assembler.push(" We have many tools", 0, 1000);
        // Still mid-sentence when it passes the limit: keep going.
        let mid = assembler.push(" and teams", 1000, LONG_TURN_MS + 200);
        assert_eq!(mid.len(), 1);
        assembler.push(".", LONG_TURN_MS + 200, LONG_TURN_MS + 300);
        let after = assembler.push(" Also", LONG_TURN_MS + 300, LONG_TURN_MS + 500);
        assert_eq!(after.len(), 2);
        assert_eq!(after[0].text, "We have many tools and teams.");
        assert!(after[0].is_final);
        assert_eq!(after[1].text, "Also");
    }

    #[test]
    fn whitespace_only_input_never_produces_a_turn() {
        let mut assembler = TurnAssembler::new(Speaker::Seller);
        assert!(assembler.push("", 0, 100).is_empty());
        assert!(assembler.push("  ", 0, 100).is_empty());
        assert_eq!(assembler.finish(), None);
    }

    #[test]
    fn wire_format_is_stable() {
        let mut assembler = TurnAssembler::new(Speaker::Customer);
        let turn = assembler.push(" Hi", 10, 20).remove(0);
        assert_eq!(
            serde_json::to_value(turn).unwrap(),
            serde_json::json!({
                "id": "customer-0", "speaker": "customer", "text": "Hi",
                "startMs": 10, "endMs": 20, "isFinal": false, "translation": null
            })
        );
    }

    #[test]
    fn an_interpretation_attaches_to_the_turn_in_progress() {
        let mut assembler = TurnAssembler::new(Speaker::Seller);
        assert_eq!(assembler.push_translation("Bonjour"), None, "nothing said yet");
        assembler.push(" Hello everyone", 0, 900);
        assembler.push_translation(" Bonjour");
        let updated = assembler.push_translation(" à tous").unwrap();
        assert_eq!(updated.translation.as_deref(), Some("Bonjour à tous"));
        assert_eq!(updated.text, "Hello everyone");
        assert!(!updated.is_final);
    }

    #[test]
    fn interpretation_arriving_after_the_turn_closed_still_reaches_it() {
        let mut assembler = TurnAssembler::new(Speaker::Seller);
        assembler.push(" Thanks for joining.", 0, 900);
        assembler.push_translation(" Merci");
        let closed = assembler.finish().unwrap();
        let late = assembler.push_translation(" de vous joindre.").unwrap();
        assert_eq!(late.id, closed.id);
        assert!(late.is_final);
        assert_eq!(late.translation.as_deref(), Some("Merci de vous joindre."));
    }
}
