//! Incremental JSONL transcript parser.
//!
//! Parses the Claude Code transcript files under `~/.claude/projects/`,
//! extracting cumulative token usage, model id, effort, last activity
//! timestamp, current working directory, and the most recent substantive
//! user prompt.
//!
//! Parsing is incremental: callers pass the previous file size, and we
//! seek to that offset before reading. This keeps polling cheap.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use serde::Deserialize;

mod ansi;
mod marker;
mod prompts;

/// Maximum bytes per JSONL line before discarding.
/// Prevents OOM from malicious files with unbounded lines.
const MAX_LINE_BYTES: usize = 10 * 1024 * 1024; // 10 MB

/// Read a line with a cap on allocation.
///
/// Uses `fill_buf`/`consume` to avoid allocating beyond the cap. Returns
/// `Ok(0)` at EOF. Overlong lines are consumed and discarded (the caller's
/// `buffer` is left empty, but a positive byte count is returned so callers
/// can distinguish from EOF).
///
/// # Errors
/// Propagates any I/O error from the underlying reader.
pub fn read_line_capped<R: Read>(
    reader: &mut BufReader<R>,
    buffer: &mut String,
) -> std::io::Result<usize> {
    let mut accumulated = Vec::new();
    let mut overflowed = false;
    let mut total_consumed = 0_usize;

    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            break;
        }

        let newline_pos = available.iter().position(|&byte| byte == b'\n');
        let chunk_end = newline_pos.map_or(available.len(), |index| index.saturating_add(1));

        if !overflowed {
            if accumulated.len().saturating_add(chunk_end) <= MAX_LINE_BYTES {
                let slice = available.get(..chunk_end).unwrap_or(&[]);
                accumulated.extend_from_slice(slice);
            } else {
                overflowed = true;
                accumulated = Vec::new();
                buffer.clear();
            }
        }

        total_consumed = total_consumed.saturating_add(chunk_end);
        reader.consume(chunk_end);

        if newline_pos.is_some() {
            break;
        }
    }

    if total_consumed == 0 {
        return Ok(0);
    }

    if !overflowed {
        *buffer = String::from_utf8(accumulated).unwrap_or_default();
    }

    Ok(total_consumed)
}

/// Inputs threaded into [`parse_jsonl`] from previous polling state.
///
/// Bundling these into one struct keeps the function signature within the
/// `too-many-arguments` budget.
pub struct ParseInputs {
    /// File size at the end of the last poll (bytes already consumed).
    pub file_size_at_last_poll: u64,
    /// Cumulative input tokens carried over from the last parse.
    pub carried_input: u64,
    /// Cumulative output tokens carried over from the last parse.
    pub carried_output: u64,
    /// Last known model id.
    pub last_model: Option<String>,
    /// Last known reasoning-effort label.
    pub last_effort: Option<String>,
    /// Last activity timestamp (ISO-8601).
    pub last_activity_ts: Option<String>,
    /// Last substantive user prompt seen.
    pub last_seen_user_prompt: Option<String>,
}

/// Parsed values extracted from a transcript.
#[derive(Debug)]
pub struct ParsedInfo {
    /// Cumulative input tokens (input + cache reads/writes).
    pub input_tokens: u64,
    /// Cumulative output tokens.
    pub output_tokens: u64,
    /// Latest model id seen.
    pub model: Option<String>,
    /// Reasoning-effort label parsed from `/model` slash-command output.
    pub effort: Option<String>,
    /// Working directory recorded by Claude Code in transcript entries.
    pub working_dir: Option<String>,
    /// Last activity timestamp (ISO-8601).
    pub last_activity: Option<String>,
    /// File size at the end of the read — feeds the next incremental call.
    pub file_size: u64,
    /// Most recent substantive user prompt.
    pub last_user_prompt: Option<String>,
}

/// Minimal serde struct for one JSONL entry.
#[derive(Deserialize)]
struct JsonlEntry {
    #[serde(default)]
    message: Option<MessageEntry>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default, rename = "cwd")]
    working_dir: Option<String>,
    #[serde(default, rename = "isMeta")]
    is_meta: Option<bool>,
    #[serde(default, rename = "toolUseResult")]
    tool_use_result: Option<serde_json::Value>,
}

/// Inner `message` field carrying model + usage.
#[derive(Deserialize)]
struct MessageEntry {
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    usage: Option<UsageEntry>,
    #[serde(default)]
    content: Option<serde_json::Value>,
}

/// Token usage counters reported per assistant message.
#[derive(Deserialize)]
struct UsageEntry {
    #[serde(default, rename = "input_tokens")]
    input: u64,
    #[serde(default, rename = "output_tokens")]
    output: u64,
    #[serde(default, rename = "cache_creation_input_tokens")]
    cache_creation_input: u64,
    #[serde(default, rename = "cache_read_input_tokens")]
    cache_read_input: u64,
}

/// Open + size-check the transcript file. Returns the open file plus its
/// current size, or a stub [`ParsedInfo`] if the file cannot be read or is
/// unchanged since the last poll.
///
/// # Errors
/// Returns `Err(stub)` (not a real I/O error) when `path` cannot be opened
/// or its size matches `file_size_at_last_poll`. Callers short-circuit by returning
/// the boxed stub directly.
fn open_and_size(path: &Path, inputs: &ParseInputs) -> Result<(File, u64), Box<ParsedInfo>> {
    let Ok(file) = File::open(path) else {
        return Err(Box::new(stub_from_inputs(inputs, 0)));
    };

    let file_size = file.metadata().map_or(0, |meta| meta.len());

    if file_size == inputs.file_size_at_last_poll && inputs.file_size_at_last_poll > 0 {
        return Err(Box::new(stub_from_inputs(inputs, file_size)));
    }

    Ok((file, file_size))
}

/// Build an unchanged-state [`ParsedInfo`] from the previous-state inputs.
fn stub_from_inputs(inputs: &ParseInputs, file_size: u64) -> ParsedInfo {
    ParsedInfo {
        input_tokens: inputs.carried_input,
        output_tokens: inputs.carried_output,
        model: inputs.last_model.clone(),
        effort: inputs.last_effort.clone(),
        working_dir: None,
        last_activity: inputs.last_activity_ts.clone(),
        file_size,
        last_user_prompt: inputs.last_seen_user_prompt.clone(),
    }
}

/// Mutable accumulator for [`parse_jsonl`].
pub struct Accumulator {
    /// Cumulative input tokens.
    pub total_input: u64,
    /// Cumulative output tokens.
    pub total_output: u64,
    /// Latest model id.
    pub model: Option<String>,
    /// Latest reasoning-effort label.
    pub effort: Option<String>,
    /// Latest activity timestamp.
    pub last_activity: Option<String>,
    /// Latest working directory recorded in a transcript entry.
    pub working_dir: Option<String>,
    /// Latest substantive user prompt.
    pub last_user_prompt: Option<String>,
}

impl Accumulator {
    fn from_inputs(inputs: &ParseInputs) -> Self {
        Self {
            total_input: inputs.carried_input,
            total_output: inputs.carried_output,
            model: inputs.last_model.clone(),
            effort: inputs.last_effort.clone(),
            last_activity: inputs.last_activity_ts.clone(),
            working_dir: None,
            last_user_prompt: inputs.last_seen_user_prompt.clone(),
        }
    }

    fn reset_for_full_reread(&mut self) {
        self.total_input = 0;
        self.total_output = 0;
        self.model = None;
        self.effort = None;
        self.last_activity = None;
        self.last_user_prompt = None;
    }

    fn into_parsed(self, file_size: u64) -> ParsedInfo {
        ParsedInfo {
            input_tokens: self.total_input,
            output_tokens: self.total_output,
            model: self.model,
            effort: self.effort,
            working_dir: self.working_dir,
            last_activity: self.last_activity,
            file_size,
            last_user_prompt: self.last_user_prompt,
        }
    }
}

/// Parse a JSONL transcript file, incrementally if possible.
pub fn parse_jsonl(path: &Path, inputs: &ParseInputs) -> ParsedInfo {
    let (file, file_size) = match open_and_size(path, inputs) {
        Ok(pair) => pair,
        Err(early) => return *early,
    };

    let mut reader = BufReader::new(file);
    let mut accumulator = Accumulator::from_inputs(inputs);

    if inputs.file_size_at_last_poll > 0 {
        let _ = reader.seek(SeekFrom::Start(inputs.file_size_at_last_poll));
    } else {
        accumulator.reset_for_full_reread();
    }

    let mut line = String::new();
    loop {
        line.clear();
        match read_line_capped(&mut reader, &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }

        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.contains("\"type\"") {
            continue;
        }

        if trimmed.contains("\"type\":\"assistant\"") {
            apply_assistant_line(trimmed, &mut accumulator);
        } else if trimmed.contains("\"type\":\"user\"") || trimmed.contains("\"type\":\"system\"") {
            apply_user_or_system_line(trimmed, &mut accumulator);
        } else {
            // Other entry types (e.g. `summary`) carry no fields we track.
        }
    }

    accumulator.into_parsed(file_size)
}

/// Process a `"type":"assistant"` JSONL line, updating accumulator fields.
fn apply_assistant_line(trimmed: &str, accumulator: &mut Accumulator) {
    // Skip synthetic entries — they have 0 tokens and overwrite real data.
    if trimmed.contains("\"<synthetic>\"") {
        return;
    }
    let Ok(entry) = serde_json::from_str::<JsonlEntry>(trimmed) else {
        return;
    };
    if let Some(timestamp) = entry.timestamp {
        accumulator.last_activity = Some(timestamp);
    }
    if entry.working_dir.is_some() {
        accumulator.working_dir = entry.working_dir;
    }
    if let Some(message) = entry.message {
        if let Some(model_name) = message.model {
            accumulator.model = Some(model_name);
        }
        if let Some(usage) = message.usage {
            accumulator.total_input = usage
                .input
                .saturating_add(usage.cache_creation_input)
                .saturating_add(usage.cache_read_input);
            accumulator.total_output = usage.output;
        }
    }
}

/// Process a `"type":"user"` or `"type":"system"` JSONL line.
fn apply_user_or_system_line(trimmed: &str, accumulator: &mut Accumulator) {
    if let Ok(entry) = serde_json::from_str::<JsonlEntry>(trimmed) {
        if let Some(timestamp) = entry.timestamp {
            accumulator.last_activity = Some(timestamp);
        }
        if !entry.is_meta.unwrap_or(false) && entry.tool_use_result.is_none() {
            if let Some(content) = entry
                .message
                .as_ref()
                .and_then(|message| message.content.as_ref())
                .and_then(serde_json::Value::as_str)
            {
                if prompts::is_substantive_prompt(content) {
                    accumulator.last_user_prompt = Some(content.to_owned());
                }
            }
        }
        if entry.working_dir.is_some() {
            accumulator.working_dir = entry.working_dir;
        }
    }

    marker::apply_set_model_marker(trimmed, accumulator);
}

#[cfg(test)]
mod tests {
    use super::{read_line_capped, MAX_LINE_BYTES};
    use std::io::{BufReader, Cursor};

    /// Two consecutive lines plus EOF.
    ///
    /// # Errors
    /// Propagates I/O failures from the reader (none in practice).
    ///
    /// # Panics
    /// Panics on assertion failure.
    #[test]
    fn read_line_capped_normal() -> std::io::Result<()> {
        let data = b"hello\nworld\n";
        let mut reader = BufReader::new(Cursor::new(data));
        let mut buffer = String::new();

        let len_a = read_line_capped(&mut reader, &mut buffer)?;
        assert!(len_a > 0);
        assert_eq!(buffer, "hello\n");

        buffer.clear();
        let len_b = read_line_capped(&mut reader, &mut buffer)?;
        assert!(len_b > 0);
        assert_eq!(buffer, "world\n");

        buffer.clear();
        let len_c = read_line_capped(&mut reader, &mut buffer)?;
        assert_eq!(len_c, 0);
        Ok(())
    }

    /// Final line without a trailing newline still parses.
    ///
    /// # Errors
    /// Propagates I/O failures from the reader (none in practice).
    ///
    /// # Panics
    /// Panics on assertion failure.
    #[test]
    fn read_line_capped_no_trailing_newline() -> std::io::Result<()> {
        let data = b"no newline";
        let mut reader = BufReader::new(Cursor::new(data));
        let mut buffer = String::new();

        let nbytes = read_line_capped(&mut reader, &mut buffer)?;
        assert!(nbytes > 0);
        assert_eq!(buffer, "no newline");
        Ok(())
    }

    /// Empty input returns `Ok(0)` (EOF) without populating the buffer.
    ///
    /// # Errors
    /// Propagates I/O failures from the reader (none in practice).
    ///
    /// # Panics
    /// Panics on assertion failure.
    #[test]
    fn read_line_capped_empty() -> std::io::Result<()> {
        let data = b"";
        let mut reader = BufReader::new(Cursor::new(data));
        let mut buffer = String::new();

        let nbytes = read_line_capped(&mut reader, &mut buffer)?;
        assert_eq!(nbytes, 0);
        assert!(buffer.is_empty());
        Ok(())
    }

    /// Overlong lines are discarded but bytes are still consumed and the
    /// next line parses correctly.
    ///
    /// # Errors
    /// Propagates I/O failures from the reader (none in practice).
    ///
    /// # Panics
    /// Panics on assertion failure.
    #[test]
    fn read_line_capped_overlong_discarded() -> std::io::Result<()> {
        let mut data = vec![b'x'; MAX_LINE_BYTES.saturating_add(100)];
        data.push(b'\n');
        data.extend_from_slice(b"ok\n");

        let mut reader = BufReader::new(Cursor::new(data));
        let mut buffer = String::new();

        let len_a = read_line_capped(&mut reader, &mut buffer)?;
        assert!(len_a > 0);
        assert!(buffer.is_empty());

        buffer.clear();
        let len_b = read_line_capped(&mut reader, &mut buffer)?;
        assert!(len_b > 0);
        assert_eq!(buffer, "ok\n");
        Ok(())
    }

    /// Overlong lines clear any stale buffer the caller passed in.
    ///
    /// # Errors
    /// Propagates I/O failures from the reader (none in practice).
    ///
    /// # Panics
    /// Panics on assertion failure.
    #[test]
    fn read_line_capped_overflow_clears_stale_buf() -> std::io::Result<()> {
        let mut data = vec![b'x'; MAX_LINE_BYTES.saturating_add(100)];
        data.push(b'\n');

        let mut reader = BufReader::new(Cursor::new(data));
        let mut buffer = String::from("stale data");

        let nbytes = read_line_capped(&mut reader, &mut buffer)?;
        assert!(nbytes > 0);
        assert!(buffer.is_empty());
        Ok(())
    }
}
