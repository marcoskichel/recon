//! Transcript reading and prompt construction for the summarizer LLM.
//!
//! Reads a Claude Code JSONL session file, filters out tool noise, and assembles a compact
//! transcript prompt for the configured backend. Also provides label normalization helpers.

use std::{
    fmt::Write as _,
    fs,
    io::{BufRead, BufReader},
    path::Path,
};

/// Maximum number of user messages to include in a prompt.
pub(super) const MAX_USER_PROMPTS: usize = 15;
/// Maximum number of assistant messages to include in a prompt.
pub(super) const MAX_ASSISTANT_TURNS: usize = 5;
/// Per-message character cap for user prompts in the transcript.
pub(super) const USER_PROMPT_CHAR_CAP: usize = 400;
/// Per-message character cap for assistant messages in the transcript.
pub(super) const ASSISTANT_CHAR_CAP: usize = 220;
/// Sentinel returned by the LLM to indicate the existing label should be kept.
pub(super) const KEEP_TOKEN: &str = "KEEP";

/// Role for one transcript turn parsed from JSONL.
pub(super) enum TurnRole {
    /// User-authored message.
    User,
    /// Assistant-authored message.
    Assistant,
}

/// One conversational turn extracted from the JSONL transcript.
pub(super) struct Turn {
    /// Role of the speaker for this turn.
    pub(super) role: TurnRole,
    /// Plain-text content for this turn (already extracted from rich blocks).
    pub(super) text: String,
}

/// Returns true if the model produced the [`KEEP_TOKEN`] sentinel.
pub(super) fn is_keep_response(raw_response: &str) -> bool {
    let trimmed = raw_response.trim().trim_matches(|byte: char| !byte.is_alphanumeric());
    trimmed.eq_ignore_ascii_case(KEEP_TOKEN)
}

/// Strip surrounding quote/backtick characters and trailing punctuation from
/// the model's raw output so it's usable as a single-line label.
pub(super) fn clean_label(text: &str) -> Option<String> {
    let mut cleaned = text
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim_matches('`')
        .trim_end_matches('.')
        .trim()
        .to_owned();

    if let Some(newline) = cleaned.find('\n') {
        cleaned.truncate(newline);
        let trimmed_len = cleaned.trim_end().len();
        cleaned.truncate(trimmed_len);
        let leading = cleaned.len().saturating_sub(cleaned.trim_start().len());
        cleaned.drain(..leading);
    }

    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

/// Counts collected while parsing a transcript.
struct TurnCounts {
    /// Number of user turns observed.
    users: usize,
    /// Number of assistant turns observed.
    assistants: usize,
}

/// Parse JSONL lines into [`Turn`]s, skipping meta/tool/empty entries.
fn parse_turns(jsonl_path: &Path) -> Option<(Vec<Turn>, TurnCounts)> {
    let file = fs::File::open(jsonl_path).ok()?;
    let reader = BufReader::new(file);

    let mut counts = TurnCounts { users: 0, assistants: 0 };
    let mut turns: Vec<Turn> = Vec::new();

    for line in reader.lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = match serde_json::from_str(&line) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };
        let kind = value.get("type").and_then(serde_json::Value::as_str).unwrap_or("");
        match kind {
            "user" => push_user_turn(&value, &mut turns, &mut counts.users),
            "assistant" => push_assistant_turn(&value, &mut turns, &mut counts.assistants),
            _ => {}
        }
    }

    Some((turns, counts))
}

/// Append a user turn to `turns` if the message is not synthetic/tool noise.
fn push_user_turn(value: &serde_json::Value, turns: &mut Vec<Turn>, count: &mut usize) {
    if value.get("isMeta").and_then(serde_json::Value::as_bool) == Some(true) {
        return;
    }
    if value.get("toolUseResult").is_some() {
        return;
    }
    let content =
        value.pointer("/message/content").and_then(serde_json::Value::as_str).unwrap_or("");
    if content.is_empty()
        || content.starts_with("<local-command")
        || content.starts_with("<command-name>")
        || content.starts_with("Caveat:")
        || content.starts_with("This session is being continued")
    {
        return;
    }
    *count = count.saturating_add(1);
    turns.push(Turn { role: TurnRole::User, text: content.to_owned() });
}

/// Append an assistant turn to `turns`, joining any rich content blocks.
fn push_assistant_turn(value: &serde_json::Value, turns: &mut Vec<Turn>, count: &mut usize) {
    let content = value.pointer("/message/content");
    let text = content.and_then(serde_json::Value::as_array).map_or_else(
        || content.and_then(serde_json::Value::as_str).map(str::to_owned).unwrap_or_default(),
        |blocks| {
            blocks
                .iter()
                .filter_map(|item| item.get("text").and_then(serde_json::Value::as_str))
                .collect::<Vec<_>>()
                .join(" ")
        },
    );
    if text.trim().is_empty() {
        return;
    }
    *count = count.saturating_add(1);
    turns.push(Turn { role: TurnRole::Assistant, text });
}

/// Counts of turns kept after applying [`MAX_USER_PROMPTS`] / [`MAX_ASSISTANT_TURNS`].
struct KeptCounts {
    /// User turns kept.
    users: usize,
    /// Assistant turns kept.
    assistants: usize,
}

/// Take the most recent N user and M assistant turns (preserving chronological order).
fn select_recent_turns(turns: &[Turn]) -> (Vec<&Turn>, KeptCounts) {
    let mut user_kept: usize = 0;
    let mut assistant_kept: usize = 0;
    let mut filtered: Vec<&Turn> = Vec::new();
    for turn in turns.iter().rev() {
        match turn.role {
            TurnRole::User => {
                if user_kept < MAX_USER_PROMPTS {
                    filtered.push(turn);
                    user_kept = user_kept.saturating_add(1);
                }
            }
            TurnRole::Assistant => {
                if assistant_kept < MAX_ASSISTANT_TURNS {
                    filtered.push(turn);
                    assistant_kept = assistant_kept.saturating_add(1);
                }
            }
        }
        if user_kept >= MAX_USER_PROMPTS && assistant_kept >= MAX_ASSISTANT_TURNS {
            break;
        }
    }
    filtered.reverse();
    (filtered, KeptCounts { users: user_kept, assistants: assistant_kept })
}

/// Render the selected turns into the transcript section of the prompt.
fn render_turns(output: &mut String, turns: &[&Turn]) {
    for turn in turns {
        let (role, char_cap) = match turn.role {
            TurnRole::User => ("USER", USER_PROMPT_CHAR_CAP),
            TurnRole::Assistant => ("ASSISTANT", ASSISTANT_CHAR_CAP),
        };
        let truncated: String = turn.text.chars().take(char_cap).collect();
        let normalized = truncated.replace(['\n', '\r'], " ");
        let _ = writeln!(output, "[{role}] {normalized}");
    }
}

/// Build the full LLM prompt from a JSONL transcript and an optional prior label.
///
/// Returns `None` when the file cannot be opened or contains no usable user turns.
pub(super) fn build_prompt(jsonl_path: &Path, previous_label: Option<&str>) -> Option<String> {
    let (turns, totals) = parse_turns(jsonl_path)?;
    if totals.users == 0 {
        return None;
    }

    let (filtered, kept) = select_recent_turns(&turns);

    let prev_label_trimmed = previous_label.map(str::trim).filter(|trimmed| !trimmed.is_empty());

    let mut output = String::new();
    if let Some(prev) = prev_label_trimmed {
        output.push_str("CURRENT LABEL: ");
        output.push_str(prev);
        output.push_str("\n\nRecent transcript (oldest first). Decide whether to KEEP or output a new label.\n\n=== TRANSCRIPT ===\n");
    } else {
        output.push_str("Recent transcript (oldest first). Output a 3-6 word label per the rules.\n\n=== TRANSCRIPT ===\n");
    }

    render_turns(&mut output, &filtered);

    let kept_users = kept.users;
    let total_users = totals.users;
    let kept_assistants = kept.assistants;
    let total_assistants = totals.assistants;
    let trailer = if prev_label_trimmed.is_some() {
        "Output exactly KEEP, or a new 3-6 word label."
    } else {
        "Output a 3-6 word label."
    };
    let _ = write!(
        output,
        "=== END TRANSCRIPT ===\n\nKept {kept_users} of {total_users} user msgs, {kept_assistants} of {total_assistants} assistant msgs.\n\n{trailer}",
    );
    Some(output)
}
