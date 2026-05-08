use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const ANTHROPIC_MODEL: &str = "claude-haiku-4-5-20251001";
const CLAUDE_CLI_DEFAULT_MODEL: &str = "claude-haiku-4-5";
const CLAUDE_CLI_DEFAULT_BINARY: &str = "claude";
const OLLAMA_DEFAULT_MODEL: &str = "gemma2:2b";
const OLLAMA_DEFAULT_URL: &str = "http://localhost:11434";
const MAX_USER_PROMPTS: usize = 15;
const MAX_ASSISTANT_TURNS: usize = 5;
const USER_PROMPT_CHAR_CAP: usize = 400;
const ASSISTANT_CHAR_CAP: usize = 220;
const MIN_DEBOUNCE_SECS: u64 = 300;
const KEEP_TOKEN: &str = "KEEP";

const CREATE_SYSTEM_PROMPT: &str = "You generate a topic label for a coding session given its recent transcript. Output ONLY the label.\n\nLABEL RULES:\n- 3-6 words.\n- Concrete nouns from the transcript: file names, function names, libraries, features, error types.\n- Action verb when natural (Add, Fix, Refactor, Wire, Build, Debug, Migrate, Tune, etc.).\n- BANNED words: coding, code, session, assistant, conversation, task, help, work, project, things, stuff.\n- No quotes. No trailing punctuation. No prefixes like 'Topic:' or 'Label:'. Output only the label itself.\n- DO NOT echo a user message verbatim — synthesize.\n\nEXAMPLES:\n  Transcript about JSON parser → Add JSON validator to body parser\n  Transcript about auth tests → Fix flaky auth-middleware tests\n  Transcript about telemetry wiring → Wire OpenTelemetry to Express server";

const UPDATE_SYSTEM_PROMPT: &str = "You decide whether an existing topic label still fits a coding session, given its recent transcript.\n\nYOU WILL RECEIVE the CURRENT LABEL plus the transcript.\n\nYOU MUST OUTPUT EXACTLY ONE OF:\n- The token KEEP (uppercase) — if the existing label still describes the current task, OR if you are not confident a new label is better.\n- A NEW LABEL — only when the topic has clearly shifted to something the existing label does not capture.\n\nNEW-LABEL RULES:\n- 3-6 words.\n- Concrete nouns from the transcript: file names, function names, libraries, features, error types.\n- Action verb when natural.\n- BANNED words: coding, code, session, assistant, conversation, task, help, work, project, things, stuff.\n- No quotes. No trailing punctuation. Output only the label itself.\n- DO NOT echo a user message verbatim — synthesize.\n\nEXAMPLES:\n  CURRENT LABEL: Fix auth tests   (topic still auth tests) →  KEEP\n  CURRENT LABEL: Fix auth tests   (topic shifted to telemetry) →  Wire OpenTelemetry to Express server\n  CURRENT LABEL: Refactor code    (vague; transcript now Postgres pool) →  Debug Postgres connection pool leak";

#[derive(Clone, Serialize, Deserialize, Debug)]
struct CachedLabel {
    file_size: u64,
    label: String,
    updated_at: u64,
}

#[derive(Clone)]
pub struct LabelStore {
    inner: Arc<Mutex<HashMap<String, CachedLabel>>>,
}

impl Default for LabelStore {
    fn default() -> Self {
        LabelStore {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl LabelStore {
    pub fn get(&self, session_id: &str) -> Option<String> {
        self.inner
            .lock()
            .ok()?
            .get(session_id)
            .map(|c| c.label.clone())
    }

    fn current_file_size(&self, session_id: &str) -> Option<u64> {
        self.inner
            .lock()
            .ok()?
            .get(session_id)
            .map(|c| c.file_size)
    }

    fn put(&self, session_id: String, cached: CachedLabel) {
        if let Ok(mut m) = self.inner.lock() {
            m.insert(session_id, cached);
        }
    }
}

#[derive(Clone)]
struct Job {
    session_id: String,
    jsonl_path: PathBuf,
    file_size: u64,
    previous_label: Option<String>,
}

enum Backend {
    Ollama { url: String, model: String },
    Anthropic { api_key: String, model: String },
    ClaudeCli { binary: String, model: String },
}

pub struct Summarizer {
    tx: Option<Sender<Job>>,
    pub store: LabelStore,
    last_enqueued: Mutex<HashMap<String, u64>>,
    enabled: bool,
}

impl Summarizer {
    pub fn start() -> Self {
        let store = LabelStore::default();
        load_cache_into(&store);

        let backend = match select_backend() {
            Some(b) => b,
            None => {
                return Summarizer {
                    tx: None,
                    store,
                    last_enqueued: Mutex::new(HashMap::new()),
                    enabled: false,
                };
            }
        };

        let (tx, rx) = channel::<Job>();
        let worker_store = store.clone();
        thread::spawn(move || worker_loop(rx, worker_store, backend));

        Summarizer {
            tx: Some(tx),
            store,
            last_enqueued: Mutex::new(HashMap::new()),
            enabled: true,
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn maybe_enqueue(&self, session_id: &str, jsonl_path: &Path, file_size: u64) {
        if !self.enabled || file_size == 0 {
            return;
        }
        let tx = match &self.tx {
            Some(t) => t,
            None => return,
        };

        let cur_size = self.store.current_file_size(session_id).unwrap_or(0);
        if cur_size == file_size {
            return;
        }

        let now = unix_now();
        if let Ok(mut state) = self.last_enqueued.lock() {
            let last = state.get(session_id).copied().unwrap_or(0);
            if now.saturating_sub(last) < MIN_DEBOUNCE_SECS {
                return;
            }
            state.insert(session_id.to_string(), now);
        }

        let previous_label = self.store.get(session_id);

        let _ = tx.send(Job {
            session_id: session_id.to_string(),
            jsonl_path: jsonl_path.to_path_buf(),
            file_size,
            previous_label,
        });
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cache_dir() -> Option<PathBuf> {
    let mut p = dirs::cache_dir()?;
    p.push("recon");
    p.push("labels");
    let _ = fs::create_dir_all(&p);
    Some(p)
}

fn load_cache_into(store: &LabelStore) {
    let dir = match cache_dir() {
        Some(d) => d,
        None => return,
    };
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let sid = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if let Ok(cached) = serde_json::from_str::<CachedLabel>(&content) {
            store.put(sid, cached);
        }
    }
}

fn select_backend() -> Option<Backend> {
    let mode = std::env::var("RECON_SUMMARIZER")
        .ok()
        .map(|s| s.to_lowercase())
        .unwrap_or_else(|| "auto".to_string());

    let ollama_url = std::env::var("RECON_OLLAMA_URL")
        .unwrap_or_else(|_| OLLAMA_DEFAULT_URL.to_string());
    let ollama_model = std::env::var("RECON_OLLAMA_MODEL")
        .unwrap_or_else(|_| OLLAMA_DEFAULT_MODEL.to_string());

    let anthropic_key = std::env::var("ANTHROPIC_API_KEY").ok().filter(|k| !k.is_empty());
    let anthropic_model = std::env::var("RECON_ANTHROPIC_MODEL")
        .unwrap_or_else(|_| ANTHROPIC_MODEL.to_string());

    let claude_binary = std::env::var("RECON_CLAUDE_BINARY")
        .unwrap_or_else(|_| CLAUDE_CLI_DEFAULT_BINARY.to_string());
    let claude_model = std::env::var("RECON_CLAUDE_MODEL")
        .unwrap_or_else(|_| CLAUDE_CLI_DEFAULT_MODEL.to_string());

    let try_ollama = || -> Option<Backend> {
        if ollama_reachable(&ollama_url) {
            Some(Backend::Ollama {
                url: ollama_url.clone(),
                model: ollama_model.clone(),
            })
        } else {
            None
        }
    };
    let try_anthropic = || -> Option<Backend> {
        anthropic_key.as_ref().map(|k| Backend::Anthropic {
            api_key: k.clone(),
            model: anthropic_model.clone(),
        })
    };
    let try_claude_cli = || -> Option<Backend> {
        if claude_cli_available(&claude_binary) {
            Some(Backend::ClaudeCli {
                binary: claude_binary.clone(),
                model: claude_model.clone(),
            })
        } else {
            None
        }
    };

    match mode.as_str() {
        "ollama" => try_ollama(),
        "anthropic" => try_anthropic(),
        "claude" | "claude-cli" | "cli" => try_claude_cli(),
        "off" | "disabled" | "none" => None,
        _ => try_claude_cli().or_else(try_ollama).or_else(try_anthropic),
    }
}

fn claude_cli_available(binary: &str) -> bool {
    std::process::Command::new(binary)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn prewarm_ollama(agent: &ureq::Agent, url: &str, model: &str) -> Option<()> {
    #[derive(Serialize)]
    struct Req<'a> {
        model: &'a str,
        keep_alive: &'a str,
    }
    let body = Req {
        model,
        keep_alive: "30m",
    };
    let endpoint = format!("{}/api/generate", url.trim_end_matches('/'));
    let _ = agent
        .post(&endpoint)
        .set("content-type", "application/json")
        .send_json(serde_json::to_value(&body).ok()?);
    Some(())
}

fn ollama_reachable(url: &str) -> bool {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_millis(500))
        .build();
    let endpoint = format!("{}/api/tags", url.trim_end_matches('/'));
    matches!(agent.get(&endpoint).call(), Ok(r) if r.status() == 200)
}

fn worker_loop(rx: Receiver<Job>, store: LabelStore, backend: Backend) {
    let agent: ureq::Agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(60))
        .build();

    if let Backend::Ollama { url, model } = &backend {
        let _ = prewarm_ollama(&agent, url, model);
    }

    while let Ok(job) = rx.recv() {
        if store.current_file_size(&job.session_id) == Some(job.file_size) {
            continue;
        }
        let prompt_text = match build_prompt(&job.jsonl_path, job.previous_label.as_deref()) {
            Some(p) => p,
            None => continue,
        };
        let has_prev = job
            .previous_label
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        let system_prompt = if has_prev {
            UPDATE_SYSTEM_PROMPT
        } else {
            CREATE_SYSTEM_PROMPT
        };
        let raw = match &backend {
            Backend::Ollama { url, model } => {
                call_ollama(&agent, url, model, system_prompt, &prompt_text)
            }
            Backend::Anthropic { api_key, model } => {
                call_anthropic(&agent, api_key, model, system_prompt, &prompt_text)
            }
            Backend::ClaudeCli { binary, model } => {
                call_claude_cli(binary, model, system_prompt, &prompt_text)
            }
        };
        let raw = match raw {
            Some(l) => l,
            None => continue,
        };

        let final_label = if has_prev && is_keep_response(&raw) {
            job.previous_label.clone().unwrap()
        } else if !has_prev && is_keep_response(&raw) {
            // Model returned KEEP for a fresh session (no prior label) — bad output.
            // Skip persisting; will retry next debounce window.
            continue;
        } else {
            raw
        };

        let cached = CachedLabel {
            file_size: job.file_size,
            label: final_label,
            updated_at: unix_now(),
        };
        if let Some(dir) = cache_dir() {
            let path = dir.join(format!("{}.json", job.session_id));
            if let Ok(s) = serde_json::to_string(&cached) {
                let _ = fs::write(path, s);
            }
        }
        store.put(job.session_id, cached);
    }
}

fn is_keep_response(raw: &str) -> bool {
    let trimmed = raw.trim().trim_matches(|c: char| !c.is_alphanumeric());
    trimmed.eq_ignore_ascii_case(KEEP_TOKEN)
}

/// One conversational turn extracted from the JSONL.
struct Turn {
    role: TurnRole,
    text: String,
}

enum TurnRole {
    User,
    Assistant,
}

fn build_prompt(jsonl_path: &Path, previous_label: Option<&str>) -> Option<String> {
    let file = fs::File::open(jsonl_path).ok()?;
    let reader = BufReader::new(file);

    let mut user_count: usize = 0;
    let mut assistant_count: usize = 0;
    let mut turns: Vec<Turn> = Vec::new();

    for line in reader.lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if ty == "user" {
            if v.get("isMeta").and_then(|m| m.as_bool()).unwrap_or(false) {
                continue;
            }
            if v.get("toolUseResult").is_some() {
                continue;
            }
            let content = v
                .pointer("/message/content")
                .and_then(|c| c.as_str())
                .unwrap_or("");
            if content.is_empty()
                || content.starts_with("<local-command")
                || content.starts_with("<command-name>")
                || content.starts_with("Caveat:")
                || content.starts_with("This session is being continued")
            {
                continue;
            }
            user_count += 1;
            turns.push(Turn {
                role: TurnRole::User,
                text: content.to_string(),
            });
        } else if ty == "assistant" {
            let text = if let Some(arr) = v.pointer("/message/content").and_then(|c| c.as_array()) {
                arr.iter()
                    .filter_map(|item| item.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join(" ")
            } else if let Some(s) = v.pointer("/message/content").and_then(|c| c.as_str()) {
                s.to_string()
            } else {
                String::new()
            };
            if text.trim().is_empty() {
                continue;
            }
            assistant_count += 1;
            turns.push(Turn {
                role: TurnRole::Assistant,
                text,
            });
        }
    }

    if user_count == 0 {
        return None;
    }

    let mut user_kept = 0usize;
    let mut assistant_kept = 0usize;
    let mut filtered: Vec<&Turn> = Vec::new();
    for turn in turns.iter().rev() {
        match turn.role {
            TurnRole::User => {
                if user_kept < MAX_USER_PROMPTS {
                    filtered.push(turn);
                    user_kept += 1;
                }
            }
            TurnRole::Assistant => {
                if assistant_kept < MAX_ASSISTANT_TURNS {
                    filtered.push(turn);
                    assistant_kept += 1;
                }
            }
        }
        if user_kept >= MAX_USER_PROMPTS && assistant_kept >= MAX_ASSISTANT_TURNS {
            break;
        }
    }
    filtered.reverse();

    let has_prev = matches!(previous_label, Some(s) if !s.trim().is_empty());

    let mut buf = String::new();
    if has_prev {
        buf.push_str("CURRENT LABEL: ");
        buf.push_str(previous_label.unwrap().trim());
        buf.push_str("\n\nRecent transcript (oldest first). Decide whether to KEEP or output a new label.\n\n=== TRANSCRIPT ===\n");
    } else {
        buf.push_str("Recent transcript (oldest first). Output a 3-6 word label per the rules.\n\n=== TRANSCRIPT ===\n");
    }

    for turn in &filtered {
        let (role, cap) = match turn.role {
            TurnRole::User => ("USER", USER_PROMPT_CHAR_CAP),
            TurnRole::Assistant => ("ASSISTANT", ASSISTANT_CHAR_CAP),
        };
        let truncated: String = turn.text.chars().take(cap).collect();
        buf.push_str(&format!(
            "[{}] {}\n",
            role,
            truncated.replace('\n', " ").replace('\r', " ")
        ));
    }

    if has_prev {
        buf.push_str(&format!(
            "=== END TRANSCRIPT ===\n\nKept {} of {} user msgs, {} of {} assistant msgs.\n\nOutput exactly KEEP, or a new 3-6 word label.",
            user_kept, user_count, assistant_kept, assistant_count,
        ));
    } else {
        buf.push_str(&format!(
            "=== END TRANSCRIPT ===\n\nKept {} of {} user msgs, {} of {} assistant msgs.\n\nOutput a 3-6 word label.",
            user_kept, user_count, assistant_kept, assistant_count,
        ));
    }
    Some(buf)
}

fn clean_label(text: &str) -> Option<String> {
    let mut cleaned = text
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim_matches('`')
        .trim_end_matches('.')
        .trim()
        .to_string();

    if let Some(idx) = cleaned.find('\n') {
        cleaned.truncate(idx);
        cleaned = cleaned.trim().to_string();
    }

    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn call_anthropic(
    agent: &ureq::Agent,
    api_key: &str,
    model: &str,
    system_prompt: &str,
    prompt: &str,
) -> Option<String> {
    #[derive(Serialize)]
    struct Req<'a> {
        model: &'a str,
        max_tokens: u32,
        system: &'a str,
        messages: Vec<Msg<'a>>,
    }
    #[derive(Serialize)]
    struct Msg<'a> {
        role: &'a str,
        content: &'a str,
    }
    #[derive(Deserialize)]
    struct Resp {
        content: Vec<RespBlock>,
    }
    #[derive(Deserialize)]
    struct RespBlock {
        text: Option<String>,
    }

    let body = Req {
        model,
        max_tokens: 48,
        system: system_prompt,
        messages: vec![Msg {
            role: "user",
            content: prompt,
        }],
    };

    let resp = agent
        .post("https://api.anthropic.com/v1/messages")
        .set("x-api-key", api_key)
        .set("anthropic-version", "2023-06-01")
        .set("content-type", "application/json")
        .send_json(serde_json::to_value(&body).ok()?)
        .ok()?;

    let parsed: Resp = resp.into_json().ok()?;
    let text: String = parsed
        .content
        .into_iter()
        .filter_map(|b| b.text)
        .collect::<Vec<_>>()
        .join("");
    clean_label(&text)
}

fn call_claude_cli(
    binary: &str,
    model: &str,
    system_prompt: &str,
    prompt: &str,
) -> Option<String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new(binary)
        .args([
            "--print",
            "--no-session-persistence",
            "--model",
            model,
            "--system-prompt",
            system_prompt,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    {
        let stdin = child.stdin.as_mut()?;
        stdin.write_all(prompt.as_bytes()).ok()?;
    }

    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    clean_label(&text)
}

fn call_ollama(
    agent: &ureq::Agent,
    url: &str,
    model: &str,
    system_prompt: &str,
    prompt: &str,
) -> Option<String> {
    #[derive(Serialize)]
    struct Req<'a> {
        model: &'a str,
        system: &'a str,
        prompt: &'a str,
        stream: bool,
        keep_alive: &'a str,
        options: Options,
    }
    #[derive(Serialize)]
    struct Options {
        num_predict: u32,
        temperature: f32,
    }
    #[derive(Deserialize)]
    struct Resp {
        response: String,
    }

    let body = Req {
        model,
        system: system_prompt,
        prompt,
        stream: false,
        keep_alive: "30m",
        options: Options {
            num_predict: 48,
            temperature: 0.2,
        },
    };

    let endpoint = format!("{}/api/generate", url.trim_end_matches('/'));
    let resp = agent
        .post(&endpoint)
        .set("content-type", "application/json")
        .send_json(serde_json::to_value(&body).ok()?)
        .ok()?;

    let parsed: Resp = resp.into_json().ok()?;
    clean_label(&parsed.response)
}
