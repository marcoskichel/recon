//! Session status detection from token counts and tmux pane content.

use std::process::Command;

use super::SessionStatus;

/// Determine session status from file recency and token counts.
///
/// Outcomes:
///   - [`SessionStatus::New`] — no tokens yet (never interacted)
///   - [`SessionStatus::Working`] — spinner visible in pane
///   - [`SessionStatus::Input`] — pane shows a permission prompt
///   - [`SessionStatus::Idle`] — anything else
pub fn determine_status(
    input_tokens: u64,
    output_tokens: u64,
    pane_target: Option<&str>,
) -> SessionStatus {
    if let Some(target) = pane_target {
        let pane = pane_status(target);
        if input_tokens == 0 && output_tokens == 0 && pane == SessionStatus::Idle {
            return SessionStatus::New;
        }
        return pane;
    }

    if input_tokens == 0 && output_tokens == 0 {
        SessionStatus::New
    } else {
        SessionStatus::Idle
    }
}

/// Determine status by inspecting the Claude Code TUI pane content.
///
/// Scans the last few non-empty lines bottom-up looking for:
///   - Working: a line starting with a Unicode spinner that also contains `…`
///   - Input: `Esc to cancel` on the last line, or a selection menu (`❯ N.`)
///   - Idle: anything else
fn pane_status(pane_target: &str) -> SessionStatus {
    let output = match Command::new("tmux").args(["capture-pane", "-t", pane_target, "-p"]).output()
    {
        Ok(success) if success.status.success() => success,
        Ok(_) | Err(_) => return SessionStatus::Idle,
    };

    let content = String::from_utf8_lossy(&output.stdout);

    let mut lines_checked: u32 = 0;
    for line in content.lines().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(found) = classify_pane_line(trimmed, lines_checked) {
            return found;
        }

        lines_checked = lines_checked.saturating_add(1);
        if lines_checked >= 10 {
            break;
        }
    }

    SessionStatus::Idle
}

/// Classify a single trimmed pane line. Returns `Some(status)` if the line
/// is decisive, `None` if we should keep scanning.
fn classify_pane_line(trimmed: &str, line_index: u32) -> Option<SessionStatus> {
    // Permission prompt on the very last non-empty line.
    if line_index == 0 && trimmed.contains("Esc to cancel") {
        return Some(SessionStatus::Input);
    }

    if let Some(first) = trimmed.chars().next() {
        if is_spinner(first) && trimmed.contains('\u{2026}') {
            return Some(SessionStatus::Working);
        }
    }

    // Selection-style permission prompts ("❯ N.")
    if let Some(found_at) = trimmed.find('\u{276F}') {
        let after_idx = found_at.saturating_add('\u{276F}'.len_utf8());
        if let Some(tail) = trimmed.get(after_idx..) {
            let trimmed_tail = tail.trim_start();
            if trimmed_tail.starts_with(|letter: char| letter.is_ascii_digit()) {
                return Some(SessionStatus::Input);
            }
        }
    }

    None
}

/// Check if a character is a Claude Code activity indicator.
///
/// Covers dingbat spinners (✽✢✳✶✻ etc.), record symbol (⏺), and middle dot
/// (·) used for progress lines.
const fn is_spinner(letter: char) -> bool {
    matches!(
        letter,
        '\u{2720}'
            ..='\u{2767}' // Dingbats: ✽✢✳✶✻✺✴✵ etc.
        | '\u{23FA}'            // ⏺ (record)
        | '\u{00B7}' // · (middle dot, used for progress)
    )
}
