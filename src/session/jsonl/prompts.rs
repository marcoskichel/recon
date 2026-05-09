//! Heuristics for deciding whether a user prompt is "substantive".
//!
//! A substantive prompt likely conveys task content rather than being a
//! continuation/affirmation/slash-command/system marker. The TUI displays
//! the most recent substantive prompt, so this filter prevents the display
//! flickering when a user just types "yes" to a permission request.

/// Returns true if a user prompt is substantive enough to display.
pub fn is_substantive_prompt(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return false;
    }
    if has_meta_prefix(trimmed) {
        return false;
    }

    let cleaned = trimmed
        .split_whitespace()
        .filter(|word| !word.starts_with("[Image") && !word.starts_with("<command-"))
        .collect::<Vec<_>>()
        .join(" ");

    let lower =
        cleaned.to_lowercase().trim_matches(|letter: char| !letter.is_alphanumeric()).to_owned();

    if STOPLIST.contains(&lower.as_str()) {
        return false;
    }

    let word_count = cleaned.split_whitespace().count();
    let char_count = cleaned.chars().count();
    word_count >= 4 || char_count >= 20
}

/// Slash-command and system markers that disqualify a prompt as substantive.
fn has_meta_prefix(trimmed: &str) -> bool {
    trimmed.starts_with("<command-name>")
        || trimmed.starts_with("<local-command-stdout>")
        || trimmed.starts_with("<local-command-caveat>")
        || trimmed.starts_with("Caveat:")
        || trimmed.starts_with("This session is being continued")
        || trimmed.starts_with("[Request interrupted")
}

/// Common interjections that get filed under the previous user prompt
/// rather than being treated as new task content.
const STOPLIST: &[&str] = &[
    "continue",
    "contiue",
    "yes",
    "y",
    "yep",
    "yeah",
    "no",
    "n",
    "nope",
    "ok",
    "okay",
    "k",
    "kk",
    "sure",
    "retry",
    "go",
    "go ahead",
    "yes go ahead",
    "yes please",
    "go for it",
    "do it",
    "fix it",
    "please",
    "thanks",
    "ty",
    "thx",
    "thank you",
    "hmm",
    "what",
    "try again",
    "looks good",
    "all good",
    "perfect",
    "great",
    "nice",
    "cool",
    "awesome",
    "i approve",
    "i apoprove",
    "approved",
    "approve",
    "keep going",
    "next",
    "more",
    "good",
];
