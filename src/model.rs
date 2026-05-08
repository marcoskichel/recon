/// Context window size for a given model ID.
pub fn context_window(model_id: &str) -> u64 {
    match model_id {
        "claude-opus-4-6" => 1_000_000,
        "claude-sonnet-4-6" => 200_000,
        "claude-sonnet-4-5-20250514" => 200_000,
        "claude-haiku-4-5-20251001" => 200_000,
        "claude-opus-4-20250514" => 200_000,
        "claude-sonnet-4-20250514" => 200_000,
        _ => 200_000,
    }
}

/// Reverse lookup: display name (from /model output) → model ID.
/// Returns None if the display name is not recognized.
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

