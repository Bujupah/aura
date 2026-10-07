use std::time::Duration;

use aura_live::ApiCredential;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

const ENDPOINT: &str = "https://api.openai.com/v1/responses";
/// Generous because a request may include web searches.
const TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, thiserror::Error)]
pub enum IntelError {
    #[error("request failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("the model service answered {status}: {message}")]
    Service { status: u16, message: String },
    #[error("the response contained no text output")]
    NoOutput,
    #[error("the response contained no image")]
    NoImage,
    #[error("the image data could not be decoded")]
    BadImage,
    #[error("the model's output did not match the schema: {0}")]
    Schema(#[from] serde_json::Error),
}

/// What a schema-constrained request needs.
pub struct StructuredRequest<'a> {
    pub instructions: &'a str,
    /// Passed as the user message: data for the model, not instructions.
    pub input: &'a str,
    pub schema_name: &'a str,
    pub schema: Value,
    /// Lets the model search the web when it judges it necessary.
    pub web_search: bool,
    /// How hard the model should think: `"low"` while the meeting is live,
    /// `"medium"` when quality matters more than speed.
    pub effort: &'static str,
}

pub struct Structured<T> {
    pub value: T,
    /// Every URL a web search returned during this request. Anything the
    /// model cites must come from this list.
    pub retrieved_urls: Vec<String>,
}

/// Schema-constrained calls to a text model through the Responses API.
#[derive(Clone)]
pub struct ResponsesClient {
    pub(crate) http: reqwest::Client,
    pub(crate) credential: ApiCredential,
    pub(crate) model: String,
}

impl ResponsesClient {
    pub fn new(credential: ApiCredential, model: impl Into<String>) -> Result<Self, IntelError> {
        Ok(Self {
            http: reqwest::Client::builder().timeout(TIMEOUT).build()?,
            credential,
            model: model.into(),
        })
    }

    pub async fn structured<T: DeserializeOwned>(
        &self,
        request: StructuredRequest<'_>,
    ) -> Result<Structured<T>, IntelError> {
        let mut body = json!({
            "model": self.model,
            // Meeting content is never retained by the provider on our behalf.
            "store": false,
            "reasoning": { "effort": request.effort },
            "instructions": request.instructions,
            "input": request.input,
            "text": { "format": {
                "type": "json_schema", "name": request.schema_name, "strict": true,
                "schema": request.schema
            }},
        });
        if request.web_search {
            body["tools"] = json!([{ "type": "web_search" }]);
            body["tool_choice"] = json!("auto");
            body["include"] = json!(["web_search_call.action.sources"]);
        }

        let payload = self.post(&body).await?;
        Ok(Structured {
            value: serde_json::from_str(output_text(&payload).ok_or(IntelError::NoOutput)?)?,
            retrieved_urls: retrieved_urls(&payload),
        })
    }
}

impl ResponsesClient {
    pub(crate) async fn post(&self, body: &Value) -> Result<Value, IntelError> {
        let response = self
            .http
            .post(ENDPOINT)
            .header("Authorization", self.credential.bearer())
            .json(body)
            .send()
            .await?;
        let status = response.status();
        let payload: Value = response.json().await?;
        if !status.is_success() {
            return Err(IntelError::Service {
                status: status.as_u16(),
                message: payload
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("no error message")
                    .to_owned(),
            });
        }
        Ok(payload)
    }
}

pub(crate) fn output_items(payload: &Value) -> impl Iterator<Item = &Value> {
    payload.get("output").and_then(Value::as_array).into_iter().flatten()
}

pub(crate) fn kind(item: &Value) -> Option<&str> {
    item.get("type").and_then(Value::as_str)
}

/// The first text part of the first message in a Responses payload.
fn output_text(payload: &Value) -> Option<&str> {
    output_items(payload)
        .filter(|item| kind(item) == Some("message"))
        .flat_map(|item| item.get("content").and_then(Value::as_array).into_iter().flatten())
        .find(|part| kind(part) == Some("output_text"))?
        .get("text")?
        .as_str()
}

fn retrieved_urls(payload: &Value) -> Vec<String> {
    output_items(payload)
        .filter(|item| kind(item) == Some("web_search_call"))
        .flat_map(|item| item.pointer("/action/sources").and_then(Value::as_array).into_iter().flatten())
        .filter_map(|source| source.get("url").and_then(Value::as_str))
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_text_after_reasoning_and_search_items() {
        let payload = json!({ "output": [
            { "type": "reasoning", "summary": [] },
            { "type": "web_search_call", "action": { "type": "search", "sources": [] } },
            { "type": "message", "content": [{ "type": "output_text", "text": "{\"windows\":[]}" }] }
        ]});
        assert_eq!(output_text(&payload), Some("{\"windows\":[]}"));
    }

    #[test]
    fn a_response_without_text_yields_nothing() {
        assert_eq!(output_text(&json!({ "output": [{ "type": "reasoning" }] })), None);
        assert_eq!(output_text(&json!({})), None);
    }

    #[test]
    fn collects_the_urls_every_search_returned() {
        let payload = json!({ "output": [
            { "type": "web_search_call", "action": { "type": "search", "sources": [
                { "type": "url", "url": "https://docs.example.com/a" },
                { "type": "url", "url": "https://docs.example.com/b" }
            ]}},
            { "type": "web_search_call", "action": { "type": "open_page" } },
            { "type": "web_search_call", "action": { "type": "search", "sources": [
                { "type": "url", "url": "https://other.example.com/c" }
            ]}},
            { "type": "message", "content": [] }
        ]});
        assert_eq!(
            retrieved_urls(&payload),
            ["https://docs.example.com/a", "https://docs.example.com/b", "https://other.example.com/c"]
        );
        assert!(retrieved_urls(&json!({ "output": [] })).is_empty());
    }
}
