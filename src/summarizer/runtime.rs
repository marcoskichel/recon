//! Worker thread loop that drains the [`LabelJob`] queue and produces labels.

use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::anthropic::{call_anthropic, AnthropicCall};
use super::backend::Backend;
use super::cache::persist_label;
use super::claude_cli::call_claude_cli;
use super::ollama::{call_ollama, prewarm_ollama, OllamaCall};
use super::prompt::{build_prompt, is_keep_response};
use super::store::{CachedLabel, LabelStore};

/// Worker timeout for one HTTP call to a backend (1 minute).
const HTTP_TIMEOUT: Duration = Duration::from_mins(1);

/// System prompt used when no prior label exists for a session.
const CREATE_SYSTEM_PROMPT: &str = "You generate a topic label for a coding session given its recent transcript. Output ONLY the label.\n\nLABEL RULES:\n- 3-6 words.\n- Concrete nouns from the transcript: file names, function names, libraries, features, error types.\n- Action verb when natural (Add, Fix, Refactor, Wire, Build, Debug, Migrate, Tune, etc.).\n- BANNED words: coding, code, session, assistant, conversation, task, help, work, project, things, stuff.\n- No quotes. No trailing punctuation. No prefixes like 'Topic:' or 'Label:'. Output only the label itself.\n- DO NOT echo a user message verbatim — synthesize.\n\nEXAMPLES:\n  Transcript about JSON parser → Add JSON validator to body parser\n  Transcript about auth tests → Fix flaky auth-middleware tests\n  Transcript about telemetry wiring → Wire OpenTelemetry to Express server";

/// System prompt used when a prior label already exists for a session.
const UPDATE_SYSTEM_PROMPT: &str = "You decide whether an existing topic label still fits a coding session, given its recent transcript.\n\nYOU WILL RECEIVE the CURRENT LABEL plus the transcript.\n\nYOU MUST OUTPUT EXACTLY ONE OF:\n- The token KEEP (uppercase) — if the existing label still describes the current task, OR if you are not confident a new label is better.\n- A NEW LABEL — only when the topic has clearly shifted to something the existing label does not capture.\n\nNEW-LABEL RULES:\n- 3-6 words.\n- Concrete nouns from the transcript: file names, function names, libraries, features, error types.\n- Action verb when natural.\n- BANNED words: coding, code, session, assistant, conversation, task, help, work, project, things, stuff.\n- No quotes. No trailing punctuation. Output only the label itself.\n- DO NOT echo a user message verbatim — synthesize.\n\nEXAMPLES:\n  CURRENT LABEL: Fix auth tests   (topic still auth tests) →  KEEP\n  CURRENT LABEL: Fix auth tests   (topic shifted to telemetry) →  Wire OpenTelemetry to Express server\n  CURRENT LABEL: Refactor code    (vague; transcript now Postgres pool) →  Debug Postgres connection pool leak";

/// One unit of work for the worker thread: relabel a single JSONL file.
#[derive(Clone)]
pub(super) struct LabelJob {
    /// JSONL session id (matches the file stem on disk).
    pub session_id: String,
    /// Absolute path to the JSONL file we should read.
    pub jsonl_path: PathBuf,
    /// File size at enqueue time (used to skip stale jobs and to record progress).
    pub file_size: u64,
    /// Existing label, if any — used to bias the model toward `KEEP`.
    pub previous_label: Option<String>,
}

/// Returns the current Unix timestamp in seconds, falling back to 0 if the
/// system clock is before the epoch.
pub(super) fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs())
}

/// Dispatch a single completion call to the configured backend.
fn invoke_backend(
    agent: &ureq::Agent,
    backend: &Backend,
    system_prompt: &str,
    prompt_text: &str,
) -> Option<String> {
    match *backend {
        Backend::Ollama { ref base_url, ref model } => {
            call_ollama(OllamaCall { agent, base_url, model, system_prompt, prompt: prompt_text })
        }
        Backend::Anthropic { ref api_key, ref model } => call_anthropic(AnthropicCall {
            agent,
            api_key,
            model,
            system_prompt,
            prompt: prompt_text,
        }),
        Backend::ClaudeCli { ref binary, ref model } => {
            call_claude_cli(binary, model, system_prompt, prompt_text)
        }
    }
}

/// Resolve the model's raw output into the final label to persist, or `None`
/// when the response should be ignored (e.g. spurious `KEEP` for a fresh session).
fn resolve_label(raw_label: String, previous_label: Option<String>) -> Option<String> {
    let has_prev =
        previous_label.as_deref().map(str::trim).is_some_and(|trimmed| !trimmed.is_empty());
    if is_keep_response(&raw_label) {
        if has_prev {
            previous_label
        } else {
            // Fresh session but model returned KEEP — bad output; retry later.
            None
        }
    } else {
        Some(raw_label)
    }
}

/// Process one [`LabelJob`] end-to-end: build prompt, call backend, persist label.
fn process_job(agent: &ureq::Agent, backend: &Backend, store: &LabelStore, work: LabelJob) {
    if store.current_file_size(&work.session_id) == Some(work.file_size) {
        return;
    }
    let Some(prompt_text) = build_prompt(&work.jsonl_path, work.previous_label.as_deref()) else {
        return;
    };
    let has_prev =
        work.previous_label.as_deref().map(str::trim).is_some_and(|trimmed| !trimmed.is_empty());
    let system_prompt = if has_prev { UPDATE_SYSTEM_PROMPT } else { CREATE_SYSTEM_PROMPT };
    let Some(raw_label) = invoke_backend(agent, backend, system_prompt, &prompt_text) else {
        return;
    };
    let Some(final_label) = resolve_label(raw_label, work.previous_label) else {
        return;
    };
    let cached =
        CachedLabel { file_size: work.file_size, label: final_label, updated_at: unix_now() };
    persist_label(&work.session_id, &cached);
    store.put(work.session_id, cached);
}

/// Drain the [`LabelJob`] receiver, processing each job with the supplied backend.
pub(super) fn worker_loop(receiver: &Receiver<LabelJob>, store: &LabelStore, backend: &Backend) {
    let agent: ureq::Agent = ureq::AgentBuilder::new().timeout(HTTP_TIMEOUT).build();

    if let Backend::Ollama { ref base_url, ref model } = *backend {
        let _ = prewarm_ollama(&agent, base_url, model);
    }

    while let Ok(work) = receiver.recv() {
        process_job(&agent, backend, store, work);
    }
}
