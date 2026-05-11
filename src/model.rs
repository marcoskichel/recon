//! Model ID metadata: context window sizes and display-name reverse lookup.

/// Default Claude context window size in tokens (200K).
const DEFAULT_CONTEXT_WINDOW: u64 = 200_000;

/// Context window size for a given model ID.
#[must_use]
pub fn context_window(model_id: &str) -> u64 {
    match model_id {
        "claude-opus-4-6" => 1_000_000,
        _ => DEFAULT_CONTEXT_WINDOW,
    }
}

/// Reverse lookup: display name (from /model output) → model ID.
/// Returns None if the display name is not recognized.
#[must_use]
pub fn id_from_display_name(display: &str) -> Option<&'static str> {
    match display {
        "Opus 4.6" | "Opus 4.6 (1M context)" => Some("claude-opus-4-6"),
        "Sonnet 4.6" => Some("claude-sonnet-4-6"),
        "Sonnet 4.5" => Some("claude-sonnet-4-5-20250514"),
        "Haiku 4.5" => Some("claude-haiku-4-5-20251001"),
        "Opus 4" => Some("claude-opus-4-20250514"),
        "Sonnet 4" => Some("claude-sonnet-4-20250514"),
        _ => None,
    }
}
