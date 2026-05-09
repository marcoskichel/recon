//! Free-standing string utilities used by the renderer:
//!
//! * [`pick_species`] — stable hash from `session_id` to species index.
//! * [`agent_display_name`] — rule for the visible agent name.
//! * [`sanitize_prompt`] — control-char scrub + whitespace squash.
//! * [`wrap_label`] — word-wrap into a fixed number of lines.
//! * [`truncate_str`] — char-aware truncation with ellipsis suffix.
//! * [`elapsed_hms`] — uptime formatter `HH:MM:SS`.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::app::App;
use crate::session::Session;

use super::types::{SPECIES_COUNT, SPECIES_NAMES};

/// FNV-1a offset basis used by [`pick_species`].
const FNV_OFFSET_BASIS: u64 = 2_166_136_261;
/// FNV-1a prime used by [`pick_species`].
const FNV_PRIME: u64 = 16_777_619;

/// Stable hash of `session_id` mapped into `0..SPECIES_COUNT`.
///
/// Uses a FNV-1a-style fold so two sessions with the same id always pick
/// the same species. The hash is intentionally distinct from
/// `session_phase_offset` so animation phase and species are uncorrelated.
#[must_use]
pub fn pick_species(session_id: &str) -> usize {
    let hash = session_id
        .bytes()
        .fold(FNV_OFFSET_BASIS, |state, byte| (state ^ u64::from(byte)).wrapping_mul(FNV_PRIME));
    let modulus = u64::try_from(SPECIES_COUNT).unwrap_or(1).max(1);
    let bucket = hash.checked_rem(modulus).unwrap_or(0);
    usize::try_from(bucket).unwrap_or(0)
}

/// Display name for the agent represented by `session`.
///
/// Falls back to the species-derived name suffixed by an instance number
/// when multiple agents share a species in the current session list.
/// A user-supplied `custom_names` entry always wins.
#[must_use]
pub fn agent_display_name(session: &Session, dashboard: &App) -> String {
    if let Some(custom) = dashboard.custom_names.get(&session.id) {
        return custom.clone();
    }
    let species = species_for(session, dashboard);
    let base_name = SPECIES_NAMES.get(species % SPECIES_COUNT).copied().unwrap_or("Agent");
    let same_before = dashboard
        .sessions
        .iter()
        .take_while(|other| other.id != session.id)
        .filter(|other| species_for(other, dashboard) == species)
        .count();
    if same_before == 0 {
        base_name.to_string()
    } else {
        let suffix = same_before.saturating_add(1);
        format!("{base_name} {suffix}")
    }
}

/// Resolve the active species for `session`: explicit assignment if any,
/// otherwise the deterministic [`pick_species`].
pub(super) fn species_for(session: &Session, dashboard: &App) -> usize {
    dashboard
        .species_assignments
        .get(&session.id)
        .copied()
        .unwrap_or_else(|| pick_species(&session.id))
}

/// Replace control characters with spaces and collapse runs of whitespace.
pub(super) fn sanitize_prompt(input: &str) -> String {
    let collapsed: String = input
        .chars()
        .map(|character| if character.is_control() { ' ' } else { character })
        .collect();
    collapsed.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Word-wrap `text` into at most `max_lines` lines of at most `max_width`
/// characters each. The last visible line is suffixed with `…` if any
/// content was truncated.
pub(super) fn wrap_label(text: &str, max_width: usize, max_lines: usize) -> Vec<String> {
    if max_width == 0 || max_lines == 0 {
        return Vec::new();
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return Vec::new();
    }

    let (initial_lines, current) = wrap_words(&words, max_width, max_lines);
    let mut output = finalize_wrap(initial_lines, current, max_lines);

    let total_chars = sum_word_chars(&words);
    let used_chars = sum_line_chars(&output);
    if used_chars < total_chars {
        ellipsize_last(&mut output, max_width);
    }
    output
}

/// Pack `words` greedily into lines of at most `max_width` chars; stop
/// when either input is exhausted or `max_lines` lines have been pushed.
fn wrap_words(words: &[&str], max_width: usize, max_lines: usize) -> (Vec<String>, String) {
    let mut lines: Vec<String> = Vec::with_capacity(max_lines);
    let mut current = String::new();
    for word in words {
        let word_chars = word.chars().count();
        let cur_chars = current.chars().count();
        let needed = if cur_chars == 0 {
            word_chars
        } else {
            cur_chars.saturating_add(1).saturating_add(word_chars)
        };

        if needed <= max_width {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
            continue;
        }

        if cur_chars > 0 {
            lines.push(std::mem::take(&mut current));
            if lines.len() == max_lines {
                break;
            }
        }

        if word_chars <= max_width {
            current.push_str(word);
        } else {
            let chunk: String = word.chars().take(max_width).collect();
            current = chunk;
        }
    }
    (lines, current)
}

/// Push the remainder buffer onto `lines` if there is still room.
fn finalize_wrap(mut lines: Vec<String>, current: String, max_lines: usize) -> Vec<String> {
    if lines.len() < max_lines && !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Sum the char count of `words` plus separators (one space between each).
fn sum_word_chars(words: &[&str]) -> usize {
    words
        .iter()
        .map(|word| word.chars().count())
        .sum::<usize>()
        .saturating_add(words.len().saturating_sub(1))
}

/// Sum the char count of `lines` plus separators (one newline between each).
fn sum_line_chars(lines: &[String]) -> usize {
    lines
        .iter()
        .map(|line| line.chars().count())
        .sum::<usize>()
        .saturating_add(lines.len().saturating_sub(1))
}

/// Append (or replace the trailing char with) `…` on the last entry of
/// `lines`, keeping each line within `max_width`.
fn ellipsize_last(lines: &mut [String], max_width: usize) {
    let Some(last) = lines.last_mut() else {
        return;
    };
    if last.chars().count() == max_width {
        let mut chars: Vec<char> = last.chars().collect();
        if chars.len() > 1 {
            chars.pop();
        }
        chars.push('\u{2026}');
        *last = chars.into_iter().collect();
    } else {
        last.push('\u{2026}');
        if last.chars().count() > max_width {
            let truncated: String = last.chars().take(max_width).collect();
            *last = truncated;
        }
    }
}

/// Truncate `value` to at most `max_width` characters, appending `…` if
/// truncation happened. Returns an empty string when `max_width <= 1`.
pub(super) fn truncate_str(value: &str, max_width: usize) -> String {
    let char_count: usize = value.chars().count();
    if char_count <= max_width {
        value.to_string()
    } else if max_width > 1 {
        let truncated: String = value.chars().take(max_width.saturating_sub(1)).collect();
        format!("{truncated}\u{2026}")
    } else {
        String::new()
    }
}

/// Format the elapsed wall-clock time since `started_at` (Unix epoch
/// seconds) as `HH:MM:SS`.
pub(super) fn elapsed_hms(started_at: u64) -> String {
    let current =
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_secs());
    let elapsed = current.saturating_sub(started_at);
    let hours = elapsed / 3600;
    let minutes = (elapsed % 3600) / 60;
    let seconds = elapsed % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}
