//! Backend selection for the summarizer LLM.
//!
//! Each enum variant carries the configuration needed to make a single
//! request via the corresponding submodule (`anthropic`, `claude_cli`, or `ollama`).

use super::anthropic::ANTHROPIC_DEFAULT_MODEL;
use super::claude_cli::{
    claude_cli_available, CLAUDE_CLI_DEFAULT_BINARY, CLAUDE_CLI_DEFAULT_MODEL,
};
use super::ollama::{ollama_reachable, OLLAMA_DEFAULT_MODEL, OLLAMA_DEFAULT_URL};

/// Configured target for an LLM completion call.
pub(super) enum Backend {
    /// Local Ollama daemon (default for developer machines that have it running).
    Ollama {
        /// Base URL (no trailing path) of the Ollama HTTP server.
        base_url: String,
        /// Ollama model identifier.
        model: String,
    },
    /// Anthropic public API.
    Anthropic {
        /// API key sent in the `x-api-key` header.
        api_key: String,
        /// Anthropic model identifier.
        model: String,
    },
    /// Local `claude` CLI binary, invoked as a subprocess.
    ClaudeCli {
        /// Path or name of the CLI binary to execute.
        binary: String,
        /// Model identifier passed via `--model`.
        model: String,
    },
}

/// Snapshot of the environment-derived backend configuration.
struct EnvConfig {
    /// `ROOSTR_SUMMARIZER` mode (lowercased).
    mode: String,
    /// `ROOSTR_OLLAMA_URL` or default.
    ollama_url: String,
    /// `ROOSTR_OLLAMA_MODEL` or default.
    ollama_model: String,
    /// `ANTHROPIC_API_KEY`, if non-empty.
    anthropic_key: Option<String>,
    /// `ROOSTR_ANTHROPIC_MODEL` or default.
    anthropic_model: String,
    /// `ROOSTR_CLAUDE_BINARY` or default.
    claude_binary: String,
    /// `ROOSTR_CLAUDE_MODEL` or default.
    claude_model: String,
}

/// Read all summarizer-relevant environment variables once.
fn read_env() -> EnvConfig {
    EnvConfig {
        mode: std::env::var("ROOSTR_SUMMARIZER")
            .ok()
            .map_or_else(|| "auto".to_owned(), |value| value.to_lowercase()),
        ollama_url: std::env::var("ROOSTR_OLLAMA_URL")
            .unwrap_or_else(|_| OLLAMA_DEFAULT_URL.to_owned()),
        ollama_model: std::env::var("ROOSTR_OLLAMA_MODEL")
            .unwrap_or_else(|_| OLLAMA_DEFAULT_MODEL.to_owned()),
        anthropic_key: std::env::var("ANTHROPIC_API_KEY").ok().filter(|value| !value.is_empty()),
        anthropic_model: std::env::var("ROOSTR_ANTHROPIC_MODEL")
            .unwrap_or_else(|_| ANTHROPIC_DEFAULT_MODEL.to_owned()),
        claude_binary: std::env::var("ROOSTR_CLAUDE_BINARY")
            .unwrap_or_else(|_| CLAUDE_CLI_DEFAULT_BINARY.to_owned()),
        claude_model: std::env::var("ROOSTR_CLAUDE_MODEL")
            .unwrap_or_else(|_| CLAUDE_CLI_DEFAULT_MODEL.to_owned()),
    }
}

/// Try to build an Ollama backend, probing the local server first.
fn try_ollama(env: &EnvConfig) -> Option<Backend> {
    if ollama_reachable(&env.ollama_url) {
        Some(Backend::Ollama { base_url: env.ollama_url.clone(), model: env.ollama_model.clone() })
    } else {
        None
    }
}

/// Try to build an Anthropic backend if a non-empty API key is available.
fn try_anthropic(env: &EnvConfig) -> Option<Backend> {
    env.anthropic_key.as_ref().map(|value| Backend::Anthropic {
        api_key: value.clone(),
        model: env.anthropic_model.clone(),
    })
}

/// Try to build a Claude-CLI backend, probing the binary first.
fn try_claude_cli(env: &EnvConfig) -> Option<Backend> {
    if claude_cli_available(&env.claude_binary) {
        Some(Backend::ClaudeCli {
            binary: env.claude_binary.clone(),
            model: env.claude_model.clone(),
        })
    } else {
        None
    }
}

/// Pick a backend based on `ROOSTR_SUMMARIZER` and the related environment
/// variables. Returns `None` if probing fails or the user disabled it.
pub(super) fn select_backend() -> Option<Backend> {
    let env = read_env();
    match env.mode.as_str() {
        "ollama" => try_ollama(&env),
        "anthropic" => try_anthropic(&env),
        "claude" | "claude-cli" | "cli" => try_claude_cli(&env),
        "off" | "disabled" | "none" => None,
        _ => try_claude_cli(&env).or_else(|| try_ollama(&env)).or_else(|| try_anthropic(&env)),
    }
}
