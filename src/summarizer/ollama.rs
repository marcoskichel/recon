//! Ollama HTTP client for the summarizer.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::prompt::clean_label;

/// Default Ollama model used when `ROOSTR_OLLAMA_MODEL` is unset.
pub(super) const OLLAMA_DEFAULT_MODEL: &str = "gemma2:2b";
/// Default Ollama base URL used when `ROOSTR_OLLAMA_URL` is unset.
pub(super) const OLLAMA_DEFAULT_URL: &str = "http://localhost:11434";

/// HTTP status code that signals the Ollama server is reachable.
const OLLAMA_OK_STATUS: u16 = 200;
/// Maximum tokens to predict in a single label completion.
const OLLAMA_NUM_PREDICT: u32 = 48;
/// Sampling temperature used when generating labels.
const OLLAMA_TEMPERATURE: f32 = 0.2;
/// `keep_alive` value sent to Ollama so the model stays warm between calls.
const OLLAMA_KEEP_ALIVE: &str = "30m";
/// Probe timeout for the reachability ping.
const PROBE_TIMEOUT: Duration = Duration::from_millis(500);

/// Configuration bundle for one [`call_ollama`] invocation.
#[derive(Clone, Copy)]
pub(super) struct OllamaCall<'data> {
    /// Pre-configured HTTP agent.
    pub agent: &'data ureq::Agent,
    /// Base URL of the Ollama server (no trailing path).
    pub base_url: &'data str,
    /// Ollama model identifier.
    pub model: &'data str,
    /// System prompt prepended to the conversation.
    pub system_prompt: &'data str,
    /// User prompt to send.
    pub prompt: &'data str,
}

/// Body for the prewarm request.
#[derive(Serialize)]
struct PrewarmBody<'data> {
    /// Model id to load.
    model: &'data str,
    /// Duration the model should stay resident.
    keep_alive: &'data str,
}

/// Generation parameters block.
#[derive(Serialize)]
struct GenerateOptions {
    /// Token cap for the response.
    num_predict: u32,
    /// Sampling temperature.
    temperature: f32,
}

/// Top-level body for the generate call.
#[derive(Serialize)]
struct GenerateBody<'data> {
    /// Model id to invoke.
    model: &'data str,
    /// System prompt sent to the model.
    system: &'data str,
    /// User prompt sent to the model.
    prompt: &'data str,
    /// Whether to stream tokens; we always want a single response.
    stream: bool,
    /// Keep the model resident for follow-up calls.
    keep_alive: &'data str,
    /// Generation tuning options.
    options: GenerateOptions,
}

/// Response shape returned by `/api/generate` when `stream=false`.
#[derive(Deserialize)]
struct GenerateResponse {
    /// Concatenated assistant response.
    response: String,
}

/// Returns `true` when the configured Ollama server responds to `/api/tags`.
pub(super) fn ollama_reachable(base_url: &str) -> bool {
    let agent = ureq::AgentBuilder::new().timeout(PROBE_TIMEOUT).build();
    let endpoint = format!("{}/api/tags", base_url.trim_end_matches('/'));
    matches!(
        agent.get(&endpoint).call(),
        Ok(response) if response.status() == OLLAMA_OK_STATUS
    )
}

/// Issue a `keep_alive` ping to nudge Ollama to load the model into memory.
pub(super) fn prewarm_ollama(agent: &ureq::Agent, base_url: &str, model: &str) -> Option<()> {
    let body = PrewarmBody { model, keep_alive: OLLAMA_KEEP_ALIVE };
    let endpoint = format!("{}/api/generate", base_url.trim_end_matches('/'));
    let _ = agent
        .post(&endpoint)
        .set("content-type", "application/json")
        .send_json(serde_json::to_value(&body).ok()?);
    Some(())
}

/// Call Ollama's `/api/generate` endpoint and return a cleaned label.
pub(super) fn call_ollama(request: OllamaCall) -> Option<String> {
    let OllamaCall { agent, base_url, model, system_prompt, prompt } = request;

    let body = GenerateBody {
        model,
        system: system_prompt,
        prompt,
        stream: false,
        keep_alive: OLLAMA_KEEP_ALIVE,
        options: GenerateOptions {
            num_predict: OLLAMA_NUM_PREDICT,
            temperature: OLLAMA_TEMPERATURE,
        },
    };

    let endpoint = format!("{}/api/generate", base_url.trim_end_matches('/'));
    let response = agent
        .post(&endpoint)
        .set("content-type", "application/json")
        .send_json(serde_json::to_value(&body).ok()?)
        .ok()?;

    let parsed: GenerateResponse = response.into_json().ok()?;
    clean_label(&parsed.response)
}
