//! Detect the `<local-command-stdout>Set model to ...` marker emitted by
//! the `/model` slash command and update model + effort accordingly.

use super::{ansi::strip_ansi, Accumulator};
use crate::model;

const MARKER: &str = "<local-command-stdout>Set model to";

/// Update `accumulator` if `trimmed` carries a `Set model to ...` marker.
pub fn apply_set_model_marker(trimmed: &str, accumulator: &mut Accumulator) {
    if !trimmed.contains(MARKER)
        || trimmed.contains("toolUseResult")
        || trimmed.contains("tool_result")
    {
        return;
    }
    let Some(stdout_pos) = trimmed.find(MARKER) else {
        return;
    };
    let tag_end = stdout_pos.saturating_add(MARKER.len());
    let Some(raw_remainder) = trimmed.get(tag_end..) else {
        return;
    };

    let truncated = raw_remainder
        .find("</local-command-stdout>")
        .and_then(|stop| raw_remainder.get(..stop))
        .unwrap_or(raw_remainder);
    let stripped = strip_ansi(truncated);
    let cleaned = stripped.trim();

    let (model_part, new_effort) = split_model_and_effort(cleaned);
    if let Some(effort_value) = new_effort {
        accumulator.effort = Some(effort_value);
    }

    let model_name = model_part
        .trim()
        .trim_end_matches("(default)")
        .trim()
        .trim_end_matches("(1M context)")
        .trim()
        .trim_end_matches("(200k context)")
        .trim();
    if let Some(canonical) = model::id_from_display_name(model_name) {
        accumulator.model = Some(canonical.to_owned());
    }
}

/// Split `"<model> with <effort> effort"` into `(model_part, Some(effort))`,
/// or `(whole, None)` if there's no `with ... effort` clause.
fn split_model_and_effort(remainder: &str) -> (&str, Option<String>) {
    let Some(with_pos) = remainder.find("with ") else {
        return (remainder, None);
    };
    let after_pos = with_pos.saturating_add("with ".len());
    let Some(after_with) = remainder.get(after_pos..) else {
        return (remainder, None);
    };
    let effort_value = after_with
        .find(" effort")
        .and_then(|stop| after_with.get(..stop))
        .map(str::trim)
        .filter(|slice| !slice.is_empty())
        .map(str::to_owned);
    let model_part = remainder.get(..with_pos).unwrap_or(remainder);
    (model_part, effort_value)
}
