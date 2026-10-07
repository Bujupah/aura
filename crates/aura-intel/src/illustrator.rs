use aura_core::topics::Topic;
use base64::Engine;
use serde_json::{json, Value};

use crate::responses::{kind, output_items};
use crate::{IntelError, ResponsesClient};

/// A sub-agent that reasons about how to picture an idea and draws it.
///
/// It is given a brief and one topic's notes, nothing else: no transcript,
/// no tools beyond image generation. What it returns is a picture for the
/// seller's eyes, labelled as an illustration, never a source of facts.
#[derive(Clone)]
pub struct Illustrator {
    client: ResponsesClient,
}

impl Illustrator {
    pub fn new(client: ResponsesClient) -> Self {
        Self { client }
    }

    /// Returns PNG bytes.
    pub async fn illustrate(&self, brief: &str, topic: &Topic) -> Result<Vec<u8>, IntelError> {
        let body = json!({
            "model": self.client.model,
            "store": false,
            "reasoning": { "effort": "low" },
            "instructions": instructions(),
            "input": json!({ "brief": brief, "title": topic.title, "notes": topic.notes }).to_string(),
            // Small and quick: the image is shown about 320 px wide.
            "tools": [{ "type": "image_generation", "size": "1024x1024", "quality": "low" }],
            "tool_choice": "required",
        });
        decode_image(&self.client.post(&body).await?)
    }
}

fn instructions() -> &'static str {
    const SOURCE: &str = include_str!("../../../prompts/visual/illustrator.md");
    SOURCE
        .strip_prefix("---")
        .and_then(|rest| rest.split_once("\n---\n"))
        .map_or(SOURCE, |(_, body)| body)
        .trim()
}

fn decode_image(payload: &Value) -> Result<Vec<u8>, IntelError> {
    let encoded = output_items(payload)
        .filter(|item| kind(item) == Some("image_generation_call"))
        .find_map(|item| item.get("result").and_then(Value::as_str))
        .ok_or(IntelError::NoImage)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| IntelError::BadImage)?;
    // Only PNG is requested; anything else is not passed on to a window.
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Ok(bytes)
    } else {
        Err(IntelError::BadImage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png() -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend_from_slice(&[0, 1, 2, 3]);
        bytes
    }

    fn payload(result: &str) -> Value {
        json!({ "output": [
            { "type": "reasoning" },
            { "type": "image_generation_call", "status": "completed", "result": result },
            { "type": "message", "content": [] }
        ]})
    }

    #[test]
    fn extracts_the_generated_png() {
        let encoded = base64::engine::general_purpose::STANDARD.encode(png());
        assert_eq!(decode_image(&payload(&encoded)).unwrap(), png());
    }

    #[test]
    fn refuses_a_response_with_no_image_or_a_non_png() {
        assert!(matches!(decode_image(&json!({ "output": [] })), Err(IntelError::NoImage)));
        assert!(matches!(decode_image(&payload("not base64 !!")), Err(IntelError::BadImage)));
        let gif = base64::engine::general_purpose::STANDARD.encode(b"GIF89a....");
        assert!(matches!(decode_image(&payload(&gif)), Err(IntelError::BadImage)));
    }

    #[test]
    fn the_prompt_is_loaded_without_its_front_matter() {
        assert!(instructions().starts_with("You illustrate one idea"));
    }
}
