//! Anthropic public-API client for the summarizer.

use serde::{Deserialize, Serialize};

use super::prompt::clean_label;

/// Default Anthropic model used when `ROOSTR_ANTHROPIC_MODEL` is unset.
pub(super) const ANTHROPIC_DEFAULT_MODEL: &str = "claude-haiku-4-5-20251001";

/// Maximum tokens we ask the API to generate for a label.
const ANTHROPIC_MAX_TOKENS: u32 = 48;
/// Anthropic API endpoint for one-shot completions.
const ANTHROPIC_ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
/// API version pin used by the Anthropic HTTP API.
const ANTHROPIC_API_VERSION: &str = "2023-06-01";

/// Configuration bundle for one [`call_anthropic`] invocation.
#[derive(Clone, Copy)]
pub(super) struct AnthropicCall<'data> {
    /// Pre-configured HTTP agent.
    pub agent: &'data ureq::Agent,
    /// Anthropic API key.
    pub api_key: &'data str,
    /// Anthropic model identifier.
    pub model: &'data str,
    /// System prompt prepended to the conversation.
    pub system_prompt: &'data str,
    /// User prompt to send.
    pub prompt: &'data str,
}

/// One chat message in an Anthropic request body.
#[derive(Serialize)]
struct ChatMessage<'data> {
    /// Role for the message (`user` or `assistant`).
    role: &'data str,
    /// Message content as a single string block.
    content: &'data str,
}

/// Top-level request body shape.
#[derive(Serialize)]
struct RequestBody<'data> {
    /// Model identifier to invoke.
    model: &'data str,
    /// Hard cap on response length.
    max_tokens: u32,
    /// System prompt prepended to the conversation.
    system: &'data str,
    /// Ordered list of chat messages.
    messages: Vec<ChatMessage<'data>>,
}

/// Single content block in the response.
#[derive(Deserialize)]
struct ResponseBlock {
    /// Text payload of the block, if any.
    text: Option<String>,
}

/// Top-level response body shape.
#[derive(Deserialize)]
struct ResponseBody {
    /// Ordered list of response content blocks.
    content: Vec<ResponseBlock>,
}

/// Send a single completion request to the Anthropic API and return a cleaned label.
pub(super) fn call_anthropic(request: AnthropicCall) -> Option<String> {
    let AnthropicCall { agent, api_key, model, system_prompt, prompt } = request;

    let body = RequestBody {
        model,
        max_tokens: ANTHROPIC_MAX_TOKENS,
        system: system_prompt,
        messages: vec![ChatMessage { role: "user", content: prompt }],
    };

    let response = agent
        .post(ANTHROPIC_ENDPOINT)
        .set("x-api-key", api_key)
        .set("anthropic-version", ANTHROPIC_API_VERSION)
        .set("content-type", "application/json")
        .send_json(serde_json::to_value(&body).ok()?)
        .ok()?;

    let parsed: ResponseBody = response.into_json().ok()?;
    let text: String = parsed.content.into_iter().filter_map(|block| block.text).collect();
    clean_label(&text)
}
